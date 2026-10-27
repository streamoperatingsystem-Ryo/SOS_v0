// Cadre SVG Tribal — encre « tatouage tribal » classique : QUE des
// courbes pleines (aucune forme géométrique, pas de ligne brisée).
// Silhouette aux coins balayés en courbes concaves (clip-path en
// quadratiques — le trou caméra/masque OBS suit la même forme), sur
// chaque arête un motif symétrique : pointe-étoile centrale rentrante,
// griffes-crocs pleines en éventail vers les coins et fines balayures
// effilées, plus des équerres de visée arrondies en retrait des coins.
// couleurFin = encre (couleur vive unifiée — noir dense conseillé),
// couleur = ton profond du dégradé ; clair = couleur éclaircie (équerres,
// lueur du voyageur).
// Tout est centré sur le tracé ou clippé dans la forme : la marge
// extérieure est rognée par overflow:hidden + clip-path du parent
// (leçon 2026-10-05) ; les décors restent dans la bande du bord, le
// centre du widget reste libre (règle #6).
// opts.animer → nœud voyageur (animateMotion path= direct — pas de
// mpath, compatible vieux CEF OBS) + respiration SMIL des griffes.

import type { CadreOpts } from "./cadre-cyberpunk";
import { angleVersCoords, assombrir, couleurOu, eclaircir, stopsLiseres } from "./couleurs";

function clampStrokeWidth(sw: number): number {
  const n = Number(sw);
  return Number.isFinite(n) ? Math.max(1, Math.min(20, n)) : 4;
}

/// Balayure d'angle (rayon de la courbe concave de la silhouette).
export function getTribalCorner(strokeWidth = 4): { cx: number; cy: number } {
  const ep = clampStrokeWidth(strokeWidth);
  return { cx: Math.max(12, Math.round(ep * 3)), cy: Math.max(9, Math.round(ep * 2.4)) };
}

/// Silhouette : rectangle aux coins remplacés par des courbes concaves
//  (Q en milieu d'angle — la « balayure » tribale).
export function tribalPath(W: number, H: number, strokeWidth = 4): string {
  const { cx, cy } = getTribalCorner(strokeWidth);
  const cx2 = Math.min(cx, W / 4);
  const cy2 = Math.min(cy, H / 4);
  return [
    `M ${cx2} 0`,
    `L ${W - cx2} 0`,
    `Q ${W - cx2 * 0.15} ${cy2 * 0.15} ${W} ${cy2}`,
    `L ${W} ${H - cy2}`,
    `Q ${W - cx2 * 0.15} ${H - cy2 * 0.15} ${W - cx2} ${H}`,
    `L ${cx2} ${H}`,
    `Q ${cx2 * 0.15} ${H - cy2 * 0.15} 0 ${H - cy2}`,
    `L 0 ${cy2}`,
    `Q ${cx2 * 0.15} ${cy2 * 0.15} ${cx2} 0`,
    `Z`,
  ].join(" ");
}

export function getTribalClipPath(
  W: number,
  H: number,
  strokeWidth = 4
): string {
  return `path('${tribalPath(W, H, strokeWidth)}')`;
}

type Pt = (t: number, prof: number) => string;

/// Pointe-étoile centrale : losange effilé à flancs concaves, pointe
//  longue rentrante ; la pointe haute sort du tracé → tronquée par le
//  clip, elle semble naître du bord (comme sur la référence).
function pointeCentrale(P: Pt, c: number, w: number, h: number): string {
  return `M ${P(c, -h * 0.2)} ` +
    `Q ${P(c + w * 0.22, h * 0.3)} ${P(c + w, h * 0.4)} ` +
    `Q ${P(c + w * 0.26, h * 0.5)} ${P(c, h)} ` +
    `Q ${P(c - w * 0.26, h * 0.5)} ${P(c - w, h * 0.4)} ` +
    `Q ${P(c - w * 0.22, h * 0.3)} ${P(c, -h * 0.2)} Z`;
}

/// Griffe-croc : base de t0 à t0+s·w sur le tracé, ventre rentrant h,
//  pointe effilée balayée jusqu'à t0+s·L. s = ±1 (sens d'éventail).
function griffe(P: Pt, t0: number, s: number, w: number, h: number, L: number): string {
  return `M ${P(t0, h * 0.06)} ` +
    `Q ${P(t0 + s * L * 0.3, h * 1.05)} ${P(t0 + s * L, h * 0.12)} ` +
    `Q ${P(t0 + s * L * 0.55, h * 0.4)} ${P(t0 + s * w, h * 0.02)} ` +
    `Z`;
}

/// Balayure : mince éclisse courbée entre t0 et t1, bombée vers
//  l'intérieur (peak) — les « filets » qui filent vers les coins.
function balayure(P: Pt, t0: number, t1: number, peak: number, ep: number): string {
  const mid = (t0 + t1) / 2;
  const th = Math.max(0.9, ep * 0.3);
  return `M ${P(t0, ep * 0.35)} ` +
    `Q ${P(mid, peak)} ${P(t1, ep * 0.3)} ` +
    `Q ${P(mid, peak + th)} ${P(t0, ep * 0.35 + th * 0.6)} ` +
    `Z`;
}

/// Motif symétrique d'une arête : étoile centrale + éventail de
//  griffes + balayures vers les coins. nx,ny = normale intérieure.
function motifBord(
  x1: number, y1: number, x2: number, y2: number,
  nx: number, ny: number, ep: number
): { corps: string; balayures: string } {
  const len = Math.hypot(x2 - x1, y2 - y1) || 1;
  const dx = (x2 - x1) / len;
  const dy = (y2 - y1) / len;
  const P: Pt = (t, prof) => `${x1 + dx * t + nx * prof} ${y1 + dy * t + ny * prof}`;
  const hMax = Math.max(9, ep * 2.4);
  const pad = hMax * 1.9;
  const demi = len / 2 - pad;
  if (demi < hMax * 1.5) return { corps: "", balayures: "" };
  const c = len / 2;
  const corps: string[] = [];
  const balayures: string[] = [];
  const sw = hMax * 0.38;
  corps.push(pointeCentrale(P, c, sw, hMax * 1.1));
  const nGriffe = demi > hMax * 3.6 ? 2 : 1;
  for (const s of [1, -1]) {
    let cur = c + s * (sw + hMax * 0.55);
    for (let i = 0; i < nGriffe; i++) {
      const k = 1 - i * 0.3;
      corps.push(griffe(P, cur, s, hMax * 0.42 * k, hMax * k, hMax * 1.45 * k + hMax * 0.4));
      cur += s * (hMax * 1.85 * k + hMax * 0.45);
    }
    const b0 = cur + s * hMax * 0.2;
    const b1 = s > 0 ? len - pad * 0.25 : pad * 0.25;
    if (Math.abs(b1 - b0) > hMax * 1.6) {
      balayures.push(balayure(P, b0, b1, -ep * 0.15, ep));
      balayures.push(balayure(P, b0 + s * hMax * 0.15, b1, hMax * 0.55, ep));
    }
  }
  return { corps: corps.join(" "), balayures: balayures.join(" ") };
}

/// Équerre de visée : petit L arrondi en retrait d'un coin.
//  sx,sy = directions intérieures depuis le coin.
function equerre(
  cx0: number, cy0: number, sx: number, sy: number,
  ix: number, iy: number, ax: number, ay: number, r: number
): string {
  const X = (v: number) => cx0 + v * sx;
  const Y = (v: number) => cy0 + v * sy;
  return `M ${X(ix + ax)} ${Y(iy)} ` +
    `L ${X(ix + r)} ${Y(iy)} ` +
    `Q ${X(ix)} ${Y(iy)} ${X(ix)} ${Y(iy + r)} ` +
    `L ${X(ix)} ${Y(iy + ay)}`;
}

export default function renderTribal(opts: CadreOpts): string {
  const { largeur, hauteur } = opts;
  const principal = couleurOu(opts.couleurFin, "#111111");
  const profond = couleurOu(opts.couleur, assombrir(principal, 0.4));
  const clair = eclaircir(principal, 0.4);
  const angle = Number.isFinite(Number(opts.gradientAngle)) ? Number(opts.gradientAngle) : 135;
  const { x1: gx1, y1: gy1, x2: gx2, y2: gy2 } = angleVersCoords(angle);
  const ep = clampStrokeWidth(opts.strokeWidth ?? 4);
  const offset = ep / 2;
  const { cx, cy } = getTribalCorner(ep);
  const cx2 = Math.min(cx, largeur / 4);
  const cy2 = Math.min(cy, hauteur / 4);

  const margin = Math.max(4, ep);
  const svgW = largeur + offset * 2 + margin * 2;
  const svgH = hauteur + offset * 2 + margin * 2;
  const viewX = -(offset + margin);
  const viewY = -(offset + margin);

  const uid = (opts.idCadre || "d").replace(/[^a-zA-Z0-9_-]/g, "-");
  const gradId = `grad-tribal-${uid}`;
  const clipId = `clip-tribal-${uid}`;

  const borderPath = tribalPath(largeur, hauteur, ep);

  // ── Motifs symétriques sur les 4 arêtes (corps pleins + balayures). ──
  const mHaut = motifBord(cx2 * 1.15, 0, largeur - cx2 * 1.15, 0, 0, 1, ep);
  const mDroite = motifBord(largeur, cy2 * 1.15, largeur, hauteur - cy2 * 1.15, -1, 0, ep);
  const mBas = motifBord(largeur - cx2 * 1.15, hauteur, cx2 * 1.15, hauteur, 0, -1, ep);
  const mGauche = motifBord(0, hauteur - cy2 * 1.15, 0, cy2 * 1.15, 1, 0, ep);
  const corps = [mHaut.corps, mDroite.corps, mBas.corps, mGauche.corps].join(" ");
  const sweeps = [mHaut.balayures, mDroite.balayures, mBas.balayures, mGauche.balayures].join(" ");
  const motif = (opts.animer
    ? `<g><animate attributeName="opacity" values="0.85;1;0.85" dur="4s" repeatCount="indefinite"/>`
    : `<g>`) +
    `<path d="${sweeps}" fill="url(#${gradId})" opacity="0.85"/>` +
    `<path d="${corps}" fill="url(#${gradId})"/></g>`;

  // ── Équerres de visée aux 4 coins (repères intérieurs arrondis). ──
  const ix = Math.min(largeur * 0.09, Math.max(cx2 * 1.6, ep * 2.6));
  const iy = Math.min(hauteur * 0.11, Math.max(cy2 * 1.6, ep * 2.6));
  const ax = Math.max(12, largeur * 0.045);
  const ay = Math.max(9, hauteur * 0.05);
  const eqW = Math.max(1.3, ep * 0.45);
  const equerres = largeur > 130 && hauteur > 90
    ? `<path d="${
        equerre(0, 0, 1, 1, ix, iy, ax, ay, 2.5) + " " +
        equerre(largeur, 0, -1, 1, ix, iy, ax, ay, 2.5) + " " +
        equerre(largeur, hauteur, -1, -1, ix, iy, ax, ay, 2.5) + " " +
        equerre(0, hauteur, 1, -1, ix, iy, ax, ay, 2.5)
      }" fill="none" stroke="${clair}" stroke-width="${eqW}" stroke-linecap="round" opacity="0.85"/>`
    : "";

  // ── Nœud voyageur (UN groupe, UN animateMotion le long du bord —
  // path= direct, pas de mpath : support universel vieux CEF). ──
  const dotR = Math.max(1.8, ep * 0.5);
  const voyageur = opts.animer
    ? `<g>` +
      `<animateMotion dur="5.5s" repeatCount="indefinite" path="${borderPath}"/>` +
      `<circle r="${dotR * 2.4}" fill="${clair}" opacity="0.35"/>` +
      `<circle r="${dotR}" fill="#ffffff"/>` +
      `</g>`
    : "";

  return `<svg width="${svgW}" height="${svgH}" viewBox="${viewX} ${viewY} ${svgW} ${svgH}" preserveAspectRatio="none" style="position:absolute; top:-${offset + margin}px; left:-${offset + margin}px; pointer-events:none; z-index:5;">
  <defs>
    <linearGradient id="${gradId}" x1="${gx1}%" y1="${gy1}%" x2="${gx2}%" y2="${gy2}%">
      ${stopsLiseres(profond, principal)}
    </linearGradient>
    <clipPath id="${clipId}">
      <path d="${borderPath}" />
    </clipPath>
  </defs>
  <g clip-path="url(#${clipId})">
    <path d="${borderPath}" fill="none" stroke="rgba(6,6,6,0.5)" stroke-width="${ep * 3.2}" stroke-linejoin="miter" />
    <path d="${borderPath}" fill="none" stroke="${principal}" stroke-width="${ep * 2.4}" stroke-linejoin="miter" opacity="0.22" />
    <path d="${borderPath}" fill="none" stroke="url(#${gradId})" stroke-width="${ep}" stroke-linejoin="miter" />
    <path d="${borderPath}" fill="none" stroke="${clair}" stroke-width="${Math.max(0.8, ep * 0.2)}" stroke-linejoin="miter" opacity="0.45" />
    ${motif}
    ${equerres}
  </g>
  ${voyageur}
</svg>`;
}
