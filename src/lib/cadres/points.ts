// Port TS de svgPathToPoints (diffusion.html) : parse un path SVG (attribut
// d) en liste de points [x, y] en aplatissant les commandes M/L/Q/C/Z en
// segments de droite. Sert à envoyer à Rust (camera_mask_sync) le polygone
// du clip-path du cadre du widget caméra — le trou destination-out de
// diffusion.html est découpé à partir du MÊME path via le même aplatissage
// (12 segments par courbe) : un seul parser des deux côtés évite toute
// dérive entre la forme du trou et la forme du masque OBS.

import { genererClipPathCadre } from "./registre";

/// Aplatit un path SVG (contenu de l'attribut d) en points [x, y].
/// `steps` = nombre de segments par courbe Q/C (défaut 12 — diffusion.html).
export function svgPathEnPoints(d: string, steps = 12): [number, number][] {
  const pts: [number, number][] = [];
  let cx = 0;
  let cy = 0;
  let startX = 0;
  let startY = 0;
  const regex = /([MLCQZ])\s*([^MLCQZ]*)/gi;
  let match: RegExpExecArray | null;
  while ((match = regex.exec(d)) !== null) {
    const cmd = match[1].toUpperCase();
    const args = match[2]
      .trim()
      .split(/[\s,]+/)
      .filter((s) => s.length > 0)
      .map(parseFloat);
    switch (cmd) {
      case "M":
        cx = args[0];
        cy = args[1];
        startX = cx;
        startY = cy;
        pts.push([cx, cy]);
        break;
      case "L":
        cx = args[0];
        cy = args[1];
        pts.push([cx, cy]);
        break;
      case "C": {
        const x0 = cx;
        const y0 = cy;
        for (let s = 1; s <= steps; s++) {
          const t = s / steps;
          const t1 = 1 - t;
          const bx =
            t1 * t1 * t1 * x0 +
            3 * t1 * t1 * t * args[0] +
            3 * t1 * t * t * args[2] +
            t * t * t * args[4];
          const by =
            t1 * t1 * t1 * y0 +
            3 * t1 * t1 * t * args[1] +
            3 * t1 * t * t * args[3] +
            t * t * t * args[5];
          pts.push([bx, by]);
        }
        cx = args[4];
        cy = args[5];
        break;
      }
      case "Q": {
        const qx0 = cx;
        const qy0 = cy;
        for (let qs = 1; qs <= steps; qs++) {
          const qt = qs / steps;
          const qt1 = 1 - qt;
          const bx = qt1 * qt1 * qx0 + 2 * qt1 * qt * args[0] + qt * qt * args[2];
          const by = qt1 * qt1 * qy0 + 2 * qt1 * qt * args[1] + qt * qt * args[3];
          pts.push([bx, by]);
        }
        cx = args[2];
        cy = args[3];
        break;
      }
      case "Z":
        pts.push([startX, startY]);
        break;
    }
  }
  return pts;
}

/// Polygone du clip-path d'un cadre (style + strokeWidth + dims du widget),
/// aplati en points widget-local — la forme exacte du trou caméra découpé
/// par diffusion.html. null = pas de forme (carre, style inconnu) → la
/// commande Rust retire les filtres, caméra rectangulaire.
export function pointsClipPathCadre(
  style: string,
  W: number,
  H: number,
  strokeWidth: number
): [number, number][] | null {
  const clip = genererClipPathCadre(style, W, H, strokeWidth);
  const m = clip && clip.match(/path\('([^']+)'\)/);
  if (!m) return null;
  const pts = svgPathEnPoints(m[1]);
  return pts.length >= 3 ? pts : null;
}
