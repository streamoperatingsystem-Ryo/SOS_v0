// Cadre SVG MGStyle — bordure tactique inspirée de Metal Gear Solid.
// Personnalisable : 2 couleurs utilisateur + angle de gradient.
// Composition extérieure uniquement : octogone biseauté, bandes de
// caution aux coins, brackets de visée, marqueurs d'angle, lignes de
// circuit sur les bords, diode radar, symbole "!" d'alerte, ticks de
// scope. AUCUN élément à l'intérieur du widget — un cadre borde,
// jamais n'obscurcit le contenu (règle AGENTS.md).

import type { CadreOpts } from "./cadre-cyberpunk";

function clampStrokeWidth(sw: number): number {
  const n = Number(sw);
  return Number.isFinite(n) ? Math.max(1, Math.min(20, n)) : 4;
}

function validColor(c: string | undefined, fallback: string): string {
  return c && /^#[0-9a-f]{6}$/i.test(c) ? c : fallback;
}

function angleToGradientCoords(angle: number): { x1: number; y1: number; x2: number; y2: number } {
  const rad = (angle * Math.PI) / 180;
  const dx = Math.cos(rad);
  const dy = Math.sin(rad);
  const scale = 50 / Math.max(Math.abs(dx), Math.abs(dy), 0.001);
  const cx = 50, cy = 50;
  return {
    x1: Math.round(cx - dx * scale),
    y1: Math.round(cy - dy * scale),
    x2: Math.round(cx + dx * scale),
    y2: Math.round(cy + dy * scale),
  };
}

export function getMGStyleClipSize(strokeWidth = 4): number {
  return Math.max(10, Math.round(clampStrokeWidth(strokeWidth) * 3));
}

export function getMGStyleClipPath(
  W: number,
  H: number,
  strokeWidth = 4,
  _variante = "codec"
): string {
  const s = Math.min(getMGStyleClipSize(strokeWidth), W / 4, H / 4);
  const sx = s;
  const sy = s * 0.7;
  return `path('M ${sx} 0 L ${W - sx} 0 L ${W} ${sy} L ${W} ${H - sy} L ${W - sx} ${H} L ${sx} ${H} L 0 ${H - sy} L 0 ${sy} Z')`;
}

export default function renderMGStyle(opts: CadreOpts): string {
  const { largeur, hauteur } = opts;
  const ep = clampStrokeWidth(opts.strokeWidth ?? 4);
  const offset = ep / 2;
  const s = Math.min(getMGStyleClipSize(ep), largeur / 4, hauteur / 4);
  const sx = s;
  const sy = s * 0.7;

  // Couleurs utilisateur : couleur = principal, couleurFin = accent/alerte.
  const cPrincipal = validColor(opts.couleur, "#ffffff");
  const cAccent = validColor(opts.couleurFin, "#000000");
  const angle = Number.isFinite(Number(opts.gradientAngle)) ? Number(opts.gradientAngle) : 135;
  const { x1: gx1, y1: gy1, x2: gx2, y2: gy2 } = angleToGradientCoords(angle);

  const margin = Math.max(10, ep * 2.5);
  const svgW = largeur + offset * 2 + margin * 2;
  const svgH = hauteur + offset * 2 + margin * 2;
  const viewX = -(offset + margin);
  const viewY = -(offset + margin);

  const uid = (opts.idCadre || "d").replace(/[^a-zA-Z0-9_-]/g, "-");
  const gradId = `grad-mg-${uid}`;
  const clipId = `clip-mg-${uid}`;
  const stripeId = `stripe-mg-${uid}`;

  const x0 = 0, y0 = 0, x1 = largeur, y1 = hauteur;
  const midY = hauteur / 2;

  const borderPath = [
    `M ${x0 + sx} ${y0}`,
    `L ${x1 - sx} ${y0}`,
    `L ${x1} ${y0 + sy}`,
    `L ${x1} ${y1 - sy}`,
    `L ${x1 - sx} ${y1}`,
    `L ${x0 + sx} ${y1}`,
    `L ${x0} ${y1 - sy}`,
    `L ${x0} ${y0 + sy}`,
    "Z",
  ].join(" ");

  // ── Triangles de caution (pattern diagonal) aux 4 coins biseautés ──
  const stripeSize = Math.max(5, ep * 1.5);
  const cornerTriangles = [
    `M ${x0} ${y0} L ${x0 + sx} ${y0} L ${x0} ${y0 + sy} Z`,
    `M ${x1} ${y0} L ${x1 - sx} ${y0} L ${x1} ${y0 + sy} Z`,
    `M ${x1} ${y1} L ${x1 - sx} ${y1} L ${x1} ${y1 - sy} Z`,
    `M ${x0} ${y1} L ${x0 + sx} ${y1} L ${x0} ${y1 - sy} Z`,
  ].map((d) => `<path d="${d}" fill="url(#${stripeId})" opacity="0.85"/>`).join("\n    ");

  // ── Brackets de visée (réticules L inversés) aux 4 coins ──
  const bracketLen = Math.max(14, ep * 4);
  const bracketGap = Math.max(4, ep * 1.2);
  const traitFin = Math.max(1, ep * 0.45);

  function bracketCoin(cx: number, cy: number, dx: number, dy: number): string {
    const X = (x: number) => cx + x * dx;
    const Y = (y: number) => cy + y * dy;
    return `M ${X(-bracketGap)} ${Y(bracketLen)} L ${X(-bracketGap)} ${Y(-bracketGap)} L ${X(bracketLen)} ${Y(-bracketGap)}`;
  }

  const brackets = [
    bracketCoin(x0, y0, 1, 1),
    bracketCoin(x1, y0, -1, 1),
    bracketCoin(x1, y1, -1, -1),
    bracketCoin(x0, y1, 1, -1),
  ].map((d) => `<path d="${d}" fill="none" stroke="${cAccent}" stroke-width="${traitFin}" stroke-linecap="square"/>`).join("");

  // ── Marqueurs d'angle (arcs aux coins biseautés) ──
  const angleMarkR = Math.max(4, ep * 1.2);
  const angleMarks = [
    { cx: x0 + sx, cy: y0, rot: 0 },
    { cx: x1 - sx, cy: y0, rot: 90 },
    { cx: x1 - sx, cy: y1, rot: 180 },
    { cx: x0 + sx, cy: y1, rot: 270 },
  ].map((m) => {
    const arcEndX = m.cx + angleMarkR * Math.cos((m.rot + 90) * Math.PI / 180);
    const arcEndY = m.cy + angleMarkR * Math.sin((m.rot + 90) * Math.PI / 180);
    return `<path d="M ${m.cx + angleMarkR} ${m.cy} A ${angleMarkR} ${angleMarkR} 0 0 1 ${arcEndX} ${arcEndY}" fill="none" stroke="${cAccent}" stroke-width="${traitFin}" opacity="0.6"/>`;
  }).join("");

  // ── Lignes de circuit (traces sur les bords extérieurs gauche/droit) ──
  const circuitStroke = Math.max(0.8, ep * 0.3);
  const circuitPad = Math.max(6, ep * 1.8);
  const circuits = [
    `M ${-circuitPad} ${sy + 20} L ${-circuitPad} ${midY - 30} L ${-circuitPad + 8} ${midY - 22} L ${-circuitPad + 8} ${midY + 22} L ${-circuitPad} ${midY + 30} L ${-circuitPad} ${hauteur - sy - 20}`,
    `M ${largeur + circuitPad} ${sy + 20} L ${largeur + circuitPad} ${midY - 30} L ${largeur + circuitPad - 8} ${midY - 22} L ${largeur + circuitPad - 8} ${midY + 22} L ${largeur + circuitPad} ${midY + 30} L ${largeur + circuitPad} ${hauteur - sy - 20}`,
  ].map((d) => `<path d="${d}" fill="none" stroke="${cAccent}" stroke-width="${circuitStroke}" opacity="0.35" stroke-linejoin="miter"/>`).join("\n    ");

  // Nœuds de circuit (petits cercles aux jonctions)
  const circuitNodes = [
    { x: -circuitPad, y: sy + 20 },
    { x: -circuitPad, y: hauteur - sy - 20 },
    { x: largeur + circuitPad, y: sy + 20 },
    { x: largeur + circuitPad, y: hauteur - sy - 20 },
    { x: -circuitPad + 8, y: midY - 22 },
    { x: -circuitPad + 8, y: midY + 22 },
    { x: largeur + circuitPad - 8, y: midY - 22 },
    { x: largeur + circuitPad - 8, y: midY + 22 },
  ].map((n) => `<circle cx="${n.x}" cy="${n.y}" r="${Math.max(1.5, ep * 0.35)}" fill="${cAccent}" opacity="0.5"/>`).join("");

  // ── Diode radar (centre haut, à l'extérieur) + halo ──
  const diodeR = Math.max(2.5, ep * 0.6);
  const diodeX = largeur / 2;
  const diodeY = -margin * 0.4;

  // ── Symbole d'alerte "!" (haut droit, à l'extérieur) ──
  const bangW = Math.max(2, ep * 0.55);
  const bangH = Math.max(12, ep * 3.5);
  const bangX = largeur + margin * 0.3;
  const bangY = -margin * 0.25;

  // ── Ticks de scope le long du bord bas (à l'extérieur, majeur au centre) ──
  const tickCount = 9;
  const tickW = Math.max(1, ep * 0.25);
  const tickH = Math.max(3, ep * 0.8);
  const tickStart = sx + 4;
  const tickEnd = largeur - sx - 4;
  const ticks = Array.from({ length: tickCount }).map((_, i) => {
    const t = tickCount > 1 ? i / (tickCount - 1) : 0.5;
    const tx = tickStart + t * (tickEnd - tickStart);
    const isMajor = i === Math.floor(tickCount / 2);
    const isMid = i % 2 === 0;
    const th = isMajor ? tickH * 1.8 : isMid ? tickH * 1.2 : tickH;
    return `<rect x="${tx - tickW / 2}" y="${hauteur + ep * 0.3}" width="${tickW}" height="${th}" fill="${cAccent}" opacity="${isMajor ? 0.9 : isMid ? 0.6 : 0.35}"/>`;
  }).join("");

  return `<svg width="${svgW}" height="${svgH}" viewBox="${viewX} ${viewY} ${svgW} ${svgH}" preserveAspectRatio="none" style="position:absolute; top:-${offset + margin}px; left:-${offset + margin}px; pointer-events:none; z-index:5;">
  <defs>
    <linearGradient id="${gradId}" x1="${gx1}%" y1="${gy1}%" x2="${gx2}%" y2="${gy2}%">
      <stop offset="0%" stop-color="${cPrincipal}" />
      <stop offset="100%" stop-color="${cAccent}" />
    </linearGradient>
    <clipPath id="${clipId}">
      <path d="${borderPath}" />
    </clipPath>
    <pattern id="${stripeId}" patternUnits="userSpaceOnUse" width="${stripeSize}" height="${stripeSize}" patternTransform="rotate(45)">
      <rect width="${stripeSize / 2}" height="${stripeSize}" fill="${cPrincipal}"/>
      <rect x="${stripeSize / 2}" width="${stripeSize / 2}" height="${stripeSize}" fill="${cAccent}"/>
    </pattern>
  </defs>
  ${cornerTriangles}
  <g clip-path="url(#${clipId})">
    <path d="${borderPath}" fill="none" stroke="${cPrincipal}" stroke-width="${ep * 2}" stroke-linejoin="miter" opacity="0.18" />
    <path d="${borderPath}" fill="none" stroke="url(#${gradId})" stroke-width="${ep}" stroke-linejoin="miter" />
    <path d="${borderPath}" fill="none" stroke="${cAccent}" stroke-width="${Math.max(0.8, ep * 0.25)}" stroke-linejoin="miter" opacity="0.55" />
  </g>
  ${brackets}
  ${angleMarks}
  ${circuits}
  ${circuitNodes}
  <circle cx="${diodeX}" cy="${diodeY}" r="${diodeR * 2}" fill="${cAccent}" opacity="0.2"/>
  <circle cx="${diodeX}" cy="${diodeY}" r="${diodeR}" fill="${cAccent}" stroke="${cPrincipal}" stroke-width="${Math.max(0.5, ep * 0.15)}"/>
  <g>
    <rect x="${bangX - bangW / 2}" y="${bangY - bangH}" width="${bangW}" height="${bangH * 0.72}" fill="${cAccent}"/>
    <rect x="${bangX - bangW / 2}" y="${bangY - bangH * 0.12}" width="${bangW}" height="${bangW}" fill="${cAccent}"/>
  </g>
  ${ticks}
</svg>`;
}
