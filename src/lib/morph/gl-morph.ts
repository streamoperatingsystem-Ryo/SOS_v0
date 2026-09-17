// Moteur de morphing WebGL — bulge/pinch radial (concept MorphLens, réécrit).
//
// PRINCIPE :
//   - Source = un canvas 2D offscreen contenant le média rendu avec la mise en
//     forme existante (fit/zoom/rotation — AUCUNE logique dupliquée ici).
//   - Le shader applique la chaîne de morphs en coordonnées normalisées (0-1)
//     du canvas : pour chaque pixel dans le rayon, on échantillonne la source
//     plus près du centre (agrandir) ou plus loin (rétrécir), avec une courbe
//     hermite (douceur fixe qui marche).
//   - ACTIVATION PARESSEUSE : le contexte GL n'est créé que si le média a des
//     morphs. Sans morph → rendu natif (img/video), zéro coût. Les clips,
//     interactions et le reste de l'app ne sont JAMAIS impactés.
//
// USAGE :
//   const r = creerRenduMorph(canvas);
//   r.rendre(source2DCanvas, morphs);  // one-shot image ou par frame vidéo
//   r.detruire();                      // libère le contexte GL

const MAX_MORPHS = 20;

const VERT = `
attribute vec2 a_pos;
varying vec2 v_uv;
void main() {
  v_uv = vec2(a_pos.x * 0.5 + 0.5, 0.5 - a_pos.y * 0.5);
  gl_Position = vec4(a_pos, 0.0, 1.0);
}
`;

// Bulge/pinch : pour chaque morph (x, y, rayon, k) — k signé :
//   k < 0 → échantillonner plus près du centre → AGRANDIR (bulge)
//   k > 0 → échantillonner plus loin → RÉTRÉCIR (pinch)
// Courbe hermite = transition douce aux bords du rayon.
// ASPECT : les distances sont calculées en "unités largeur" (d.y / aspect)
// → le cercle de morphing est CIRCULAIRE en pixels quel que soit le ratio
// du canvas (sinon il serait elliptique en espace UV sur un canvas 16:9).
const FRAG = `
precision mediump float;
varying vec2 v_uv;
uniform sampler2D u_tex;
uniform int u_count;
uniform float u_aspect;
uniform vec4 u_morphs[${MAX_MORPHS}];
void main() {
  vec2 p = v_uv;
  for (int i = 0; i < ${MAX_MORPHS}; i++) {
    if (i >= u_count) break;
    vec4 m = u_morphs[i];
    vec2 d = p - m.xy;
    d.y /= u_aspect;
    float dist = length(d);
    if (dist < m.z && dist > 0.0001) {
      float infl = 1.0 - dist / m.z;
      float smoothInfl = infl * infl * (3.0 - 2.0 * infl);
      float deform = dist * (1.0 + m.w * smoothInfl);
      vec2 dir = d / dist;
      p = m.xy + vec2(dir.x, dir.y * u_aspect) * deform;
    }
  }
  gl_FragColor = texture2D(u_tex, p);
}
`;

export interface RenduMorph {
  rendre: (source: HTMLCanvasElement, morphs: { x: number; y: number; rayon: number; intensite: number; mode: string }[]) => void;
  detruire: () => void;
}

/// Construit la chaîne `filter` CSS (et `ctx.filter` canvas) à partir des 5
/// effets visuels. Retourne "" si tous neutres → pas de filter appliqué
/// (zéro coût). La pixelisation n'est PAS gérée ici (pas d'équivalent CSS
/// pur) — elle est appliquée séparément en canvas (downscale/upscale).
/// `lum`/`contraste` : -100..100 (0 = neutre). `teinte` : 0..360 deg.
/// `flou` : 0..20 px (0 = neutre).
export function chaineFilter(lum: number, contraste: number, teinte: number, flou: number): string {
  const parts: string[] = [];
  if (lum) parts.push(`brightness(${1 + lum / 100})`);
  if (contraste) parts.push(`contrast(${1 + contraste / 100})`);
  if (teinte) parts.push(`hue-rotate(${teinte}deg)`);
  if (flou > 0) parts.push(`blur(${flou}px)`);
  return parts.join(" ");
}

/// Vrai si au moins un effet est non-neutre (y compris pixelisation).
export function aEffets(lum: number, contraste: number, teinte: number, flou: number, pixel: number): boolean {
  return lum !== 0 || contraste !== 0 || teinte !== 0 || flou > 0 || pixel > 0;
}

/// object-fit en canvas (réplique le CSS object-fit des médias) — partagé par
/// Widget.svelte et Canvas.svelte pour le rendu offscreen des médias morphés.
export function dessinerFitCanvas(
  ctx: CanvasRenderingContext2D,
  src: CanvasImageSource,
  sw: number,
  sh: number,
  dw: number,
  dh: number,
  fit: string
) {
  if (fit === "fill") {
    ctx.drawImage(src, 0, 0, sw, sh, 0, 0, dw, dh);
    return;
  }
  let scale: number;
  if (fit === "cover") scale = Math.max(dw / sw, dh / sh);
  else if (fit === "none") scale = 1;
  else if (fit === "scale-down") scale = Math.min(1, Math.min(dw / sw, dh / sh));
  else scale = Math.min(dw / sw, dh / sh); // contain (défaut)
  const dW = sw * scale;
  const dH = sh * scale;
  const dx = (dw - dW) / 2;
  const dy = (dh - dH) / 2;
  if (dx < 0 || dy < 0 || dW > dw || dH > dh) {
    ctx.beginPath();
    ctx.rect(0, 0, dw, dh);
    ctx.clip();
  }
  ctx.drawImage(src, 0, 0, sw, sh, dx, dy, dW, dH);
}

function compiler(gl: WebGLRenderingContext, type: number, src: string): WebGLShader {
  const sh = gl.createShader(type)!;
  gl.shaderSource(sh, src);
  gl.compileShader(sh);
  if (!gl.getShaderParameter(sh, gl.COMPILE_STATUS)) {
    const log = gl.getShaderInfoLog(sh);
    gl.deleteShader(sh);
    throw new Error("Compile shader: " + log);
  }
  return sh;
}

/// Crée le rendu GL sur le canvas cible. Throw si WebGL indisponible
/// (l'appelant retombe alors sur le rendu natif).
export function creerRenduMorph(canvas: HTMLCanvasElement): RenduMorph {
  const gl = (canvas.getContext("webgl", {
    preserveDrawingBuffer: true,
    alpha: true,
    premultipliedAlpha: false,
  }) ?? canvas.getContext("experimental-webgl")) as WebGLRenderingContext | null;
  if (!gl) throw new Error("WebGL indisponible");

  const prog = gl.createProgram()!;
  gl.attachShader(prog, compiler(gl, gl.VERTEX_SHADER, VERT));
  gl.attachShader(prog, compiler(gl, gl.FRAGMENT_SHADER, FRAG));
  gl.linkProgram(prog);
  if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) {
    throw new Error("Link program: " + gl.getProgramInfoLog(prog));
  }
  gl.useProgram(prog);

  // Quad plein écran (2 triangles).
  const buf = gl.createBuffer();
  gl.bindBuffer(gl.ARRAY_BUFFER, buf);
  gl.bufferData(
    gl.ARRAY_BUFFER,
    new Float32Array([-1, -1, 1, -1, -1, 1, -1, 1, 1, -1, 1, 1]),
    gl.STATIC_DRAW
  );
  const aPos = gl.getAttribLocation(prog, "a_pos");
  gl.enableVertexAttribArray(aPos);
  gl.vertexAttribPointer(aPos, 2, gl.FLOAT, false, 0, 0);

  const uTex = gl.getUniformLocation(prog, "u_tex");
  const uCount = gl.getUniformLocation(prog, "u_count");
  const uAspect = gl.getUniformLocation(prog, "u_aspect");
  const uMorphs = gl.getUniformLocation(prog, "u_morphs");
  const tex = gl.createTexture();
  gl.bindTexture(gl.TEXTURE_2D, tex);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
  gl.uniform1i(uTex, 0);

  const data = new Float32Array(MAX_MORPHS * 4);

  return {
    /// Rend source (canvas 2D = média mis en forme) + chaîne de morphs → canvas.
    rendre(source, morphs) {
      if (canvas.width !== source.width || canvas.height !== source.height) {
        canvas.width = source.width;
        canvas.height = source.height;
      }
      gl.viewport(0, 0, canvas.width, canvas.height);

      // Texture = le média mis en forme (fit/zoom/rot déjà appliqués en 2D).
      gl.bindTexture(gl.TEXTURE_2D, tex);
      gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, source);

      // Chaîne de morphs → uniforms (k signé : agrandir échantillonne plus près).
      const n = Math.min(morphs.length, MAX_MORPHS);
      for (let i = 0; i < n; i++) {
        const m = morphs[i];
        const k =
          m.mode === "retrecir"
            ? m.intensite / 100
            : -(m.intensite / 200);
        data[i * 4] = m.x;
        data[i * 4 + 1] = m.y;
        data[i * 4 + 2] = m.rayon;
        data[i * 4 + 3] = k;
      }
      gl.uniform1i(uCount, n);
      gl.uniform1f(uAspect, canvas.width / Math.max(1, canvas.height));
      gl.uniform4fv(uMorphs, data);

      gl.clearColor(0, 0, 0, 0);
      gl.clear(gl.COLOR_BUFFER_BIT);
      gl.drawArrays(gl.TRIANGLES, 0, 6);
    },

    detruire() {
      const ext = gl.getExtension("WEBGL_lose_context");
      ext?.loseContext();
    },
  };
}
