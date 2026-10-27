// Cadre SVG Hexagone — bordure octogonale + identité « ruche sci-fi » :
// grappes d'alvéoles dans la marge des coins, nœuds lumineux aux sommets,
// glyphes hexagonaux aux milieux d'arêtes. Tons dérivés des couleurs
// utilisateur : couleurFin = principal (couleur vive unifiée), couleur =
// profond ; clair = couleur éclaircie (aucune palette figée).
// opts.animer → les 4 grappes d'alvéoles tournent lentement sur
// elles-mêmes (UN animateTransform par grappe, transform = pas cher).

import type { CadreOpts } from "./cadre-cyberpunk";
import { angleVersCoords, assombrir, couleurOu, eclaircir, stopsLiseres } from "./couleurs";

function clampStrokeWidth(sw: number): number {
  const n = Number(sw);
  return Number.isFinite(n) ? Math.max(1, Math.min(20, n)) : 4;
}

export function getHexagonClipSize(strokeWidth = 4): number {
  return Math.max(12, Math.round(clampStrokeWidth(strokeWidth) * 4));
}

export function getHexagonClipPath(
  W: number,
  H: number,
  strokeWidth = 4
): string {
  const s = Math.min(getHexagonClipSize(strokeWidth), W / 4, H / 4);
  const parts = [
    `M ${s} 0`,
    `L ${W - s} 0`,
    `L ${W} ${s * 0.5}`,
    `L ${W} ${H - s * 0.5}`,
    `L ${W - s} ${H}`,
    `L ${s} ${H}`,
    `L 0 ${H - s * 0.5}`,
    `L 0 ${s * 0.5}`,
    `Z`,
  ];
  return `path('${parts.join(" ")}')`;
}

function hexPath(cx: number, cy: number, r: number): string {
  const pts: string[] = [];
  for (let i = 0; i < 6; i++) {
    const angle = (Math.PI / 3) * i - Math.PI / 6;
    pts.push(`${cx + r * Math.cos(angle)} ${cy + r * Math.sin(angle)}`);
  }
  return `M ${pts.join(" L ")} Z`;
}

export default function renderHexagon(opts: CadreOpts): string {
  const { largeur, hauteur } = opts;
  const principal = couleurOu(opts.couleurFin, "#ffffff");
  const p = {
    principal,
    profond: couleurOu(opts.couleur, assombrir(principal, 0.3)),
    clair: eclaircir(principal, 0.45),
  };
  const angle = Number.isFinite(Number(opts.gradientAngle)) ? Number(opts.gradientAngle) : 135;
  const { x1: gx1, y1: gy1, x2: gx2, y2: gy2 } = angleVersCoords(angle);
  const ep = clampStrokeWidth(opts.strokeWidth ?? 4);
  const s = Math.min(getHexagonClipSize(ep), largeur / 4, hauteur / 4);

  const margin = Math.max(10, ep * 2.6);
  const svgW = largeur + margin * 2;
  const svgH = hauteur + margin * 2;
  const vx = -margin;
  const vy = -margin;

  const uid = (opts.idCadre || "d").replace(/[^a-zA-Z0-9_-]/g, "-");
  const gradId = `grad-hex-${uid}`;
  const clipId = `clip-hex-${uid}`;

  const borderPath = [
    `M ${s} 0`,
    `L ${largeur - s} 0`,
    `L ${largeur} ${s * 0.5}`,
    `L ${largeur} ${hauteur - s * 0.5}`,
    `L ${largeur - s} ${hauteur}`,
    `L ${s} ${hauteur}`,
    `L 0 ${hauteur - s * 0.5}`,
    `L 0 ${s * 0.5}`,
    `Z`,
  ].join(" ");

  // ── Grappes d'alvéoles aux 4 coins (marge extérieure) : un hexagone
  // principal + 2 satellites chaînés, centrés hors du widget. ──
  const hexR = Math.max(5, ep * 1.4);
  const clusterD = s * 0.9 + hexR * 0.9;
  const clusters = [
    { cx: -clusterD * 0.4, cy: -clusterD * 0.4 },
    { cx: largeur + clusterD * 0.4, cy: -clusterD * 0.4 },
    { cx: largeur + clusterD * 0.4, cy: hauteur + clusterD * 0.4 },
    { cx: -clusterD * 0.4, cy: hauteur + clusterD * 0.4 },
  ];
  const traitFin = Math.max(1, ep * 0.35);
  const clustersSvg = clusters
    .map((c, i) => {
      const inner =
        `<path d="${hexPath(c.cx, c.cy, hexR)}" fill="${p.profond}" fill-opacity="0.25" stroke="${p.clair}" stroke-width="${traitFin}"/>` +
        `<path d="${hexPath(c.cx + hexR * 1.9, c.cy, hexR * 0.55)}" fill="none" stroke="${p.principal}" stroke-width="${traitFin * 0.7}" opacity="0.8"/>` +
        `<path d="${hexPath(c.cx - hexR * 0.95, c.cy + hexR * 1.65, hexR * 0.55)}" fill="none" stroke="${p.principal}" stroke-width="${traitFin * 0.7}" opacity="0.8"/>`;
      const anim = opts.animer
        ? `<animateTransform attributeName="transform" type="rotate" from="0 ${c.cx} ${c.cy}" to="${i % 2 === 0 ? 360 : -360} ${c.cx} ${c.cy}" dur="10s" repeatCount="indefinite"/>`
        : "";
      return `<g>${anim}${inner}</g>`;
    })
    .join("");

  // ── Nœuds aux 8 sommets du bord (points du clip-path) ──
  const sommets = [
    [s, 0], [largeur - s, 0],
    [largeur, s * 0.5], [largeur, hauteur - s * 0.5],
    [largeur - s, hauteur], [s, hauteur],
    [0, hauteur - s * 0.5], [0, s * 0.5],
  ];
  const noeudR = Math.max(1.2, ep * 0.3);
  const noeuds = sommets
    .map(([nx, ny]) => `<circle cx="${nx}" cy="${ny}" r="${noeudR}" fill="${p.clair}"/>`)
    .join("");

  // ── Glyphes hexagonaux aux milieux des 4 arêtes (marge) ──
  const glypheR = hexR * 0.65;
  const glyphes = [
    hexPath(largeur / 2, -margin * 0.45, glypheR),
    hexPath(largeur / 2, hauteur + margin * 0.45, glypheR),
    hexPath(-margin * 0.45, hauteur / 2, glypheR),
    hexPath(largeur + margin * 0.45, hauteur / 2, glypheR),
  ]
    .map((d) => `<path d="${d}" fill="none" stroke="${p.clair}" stroke-width="${traitFin * 0.8}" opacity="0.5"/>`)
    .join("");

  // ── Nœud voyageur (UN groupe, UN animateMotion le long du bord
  // hexagonal — path= direct, pas de mpath : support universel). ──
  const voyageur = opts.animer
    ? `<g>` +
      `<animateMotion dur="6s" repeatCount="indefinite" path="${borderPath}"/>` +
      `<circle r="${Math.max(2.2, ep * 0.5)}" fill="${p.clair}" opacity="0.4"/>` +
      `<circle r="${Math.max(1.4, ep * 0.32)}" fill="#ffffff"/>` +
      `</g>`
    : "";

  return `<svg width="${svgW}" height="${svgH}" viewBox="${vx} ${vy} ${svgW} ${svgH}" preserveAspectRatio="none" style="position:absolute; top:-${margin}px; left:-${margin}px; pointer-events:none; z-index:5;">
  <defs>
    <linearGradient id="${gradId}" x1="${gx1}%" y1="${gy1}%" x2="${gx2}%" y2="${gy2}%">
      ${stopsLiseres(p.profond, p.principal)}
    </linearGradient>
    <clipPath id="${clipId}">
      <path d="${borderPath}" />
    </clipPath>
  </defs>
  <g clip-path="url(#${clipId})">
    <path d="${borderPath}" fill="none" stroke="${p.principal}" stroke-width="${ep * 2.2}" stroke-linejoin="miter" opacity="0.2" />
    <path d="${borderPath}" fill="none" stroke="url(#${gradId})" stroke-width="${ep}" stroke-linejoin="miter" />
    <path d="${borderPath}" fill="none" stroke="${p.clair}" stroke-width="${Math.max(1, ep * 0.3)}" stroke-linejoin="miter" opacity="0.6" />
  </g>
  ${clustersSvg}
  ${glyphes}
  <g opacity="0.85">
    ${noeuds}
  </g>
  ${voyageur}
  <path d="${borderPath}" fill="none" stroke="${p.clair}" stroke-width="${Math.max(1, ep * 0.2)}" stroke-linejoin="miter" opacity="0.4" transform="translate(${ep * 0.6} ${ep * 0.6}) scale(${Math.max(0.01, 1 - (ep * 1.2) / largeur)} ${Math.max(0.01, 1 - (ep * 1.2) / hauteur)})" />
</svg>`;
}
