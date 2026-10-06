// Cadre SVG Thin Line — minimalisme premium : double filet (trait
// principal + filet externe décalé), accents L aux coins, point de
// « fermoir » signature. Tons dérivés des couleurs utilisateur :
// couleur = trait principal, couleurFin = accent (défaut : éclaircie).
// opts.animer → un segment comète balaie le bord (stroke-dashoffset SMIL
// sur pathLength=100 — un seul attribut animé, le moins énergivore).

import type { CadreOpts } from "./cadre-cyberpunk";
import { couleurOu, eclaircir } from "./couleurs";

function clampStrokeWidth(sw: number): number {
  const n = Number(sw);
  return Number.isFinite(n) ? Math.max(1, Math.min(20, n)) : 4;
}

export function getThinLineClipSize(strokeWidth = 4): number {
  return Math.max(6, Math.round(clampStrokeWidth(strokeWidth) * 1.5));
}

export function getThinLineClipPath(
  W: number,
  H: number,
  strokeWidth = 4
): string {
  const r = getThinLineClipSize(strokeWidth);
  const rEff = Math.min(r, W / 4, H / 4);
  return `path('M ${rEff} 0 L ${W - rEff} 0 Q ${W} 0 ${W} ${rEff} L ${W} ${H - rEff} Q ${W} ${H} ${W - rEff} ${H} L ${rEff} ${H} Q 0 ${H} 0 ${H - rEff} L 0 ${rEff} Q 0 0 ${rEff} 0 Z')`;
}

export default function renderThinLine(opts: CadreOpts): string {
  const { largeur, hauteur } = opts;
  const p = {
    principal: couleurOu(opts.couleur, "#ffffff"),
    accent: couleurOu(opts.couleurFin, eclaircir(couleurOu(opts.couleur, "#ffffff"), 0.3)),
  };
  const ep = clampStrokeWidth(opts.strokeWidth ?? 4);
  const r = Math.min(getThinLineClipSize(ep), largeur / 4, hauteur / 4);

  // Marge assez grande pour le double filet externe + les accents L.
  const filetGap = Math.max(3, ep * 1.1);
  const margin = Math.max(6, ep * 1.5);
  const svgW = largeur + margin * 2;
  const svgH = hauteur + margin * 2;
  const vx = -margin;
  const vy = -margin;

  const cornerLen = Math.max(10, ep * 3);
  const cornerGap = filetGap + Math.max(2, ep * 0.5);

  // Path du bord (pour le segment comète — pathLength=100 normalise).
  const borderPath = `M ${r} 0 L ${largeur - r} 0 Q ${largeur} 0 ${largeur} ${r} L ${largeur} ${hauteur - r} Q ${largeur} ${hauteur} ${largeur - r} ${hauteur} L ${r} ${hauteur} Q 0 ${hauteur} 0 ${hauteur - r} L 0 ${r} Q 0 0 ${r} 0 Z`;

  // Segment comète : un trait de ~7% du périmètre qui tourne en boucle.
  const comet = opts.animer
    ? `<path d="${borderPath}" fill="none" stroke="${p.accent}" stroke-width="${Math.max(1.2, ep * 0.7)}" stroke-linecap="round" pathLength="100" stroke-dasharray="7 93" stroke-dashoffset="0">` +
      `<animate attributeName="stroke-dashoffset" from="0" to="-100" dur="4s" repeatCount="indefinite"/>` +
      `</path>`
    : "";

  // Point de « fermoir » : un seul accent posé sur le filet, milieu haut.
  const fermoirR = Math.max(1.6, ep * 0.45);

  return `<svg width="${svgW}" height="${svgH}" viewBox="${vx} ${vy} ${svgW} ${svgH}" preserveAspectRatio="none" style="position:absolute; top:-${margin}px; left:-${margin}px; pointer-events:none; z-index:5;">
  <rect x="0" y="0" width="${largeur}" height="${hauteur}" rx="${r}" ry="${r}" fill="none" stroke="${p.principal}" stroke-width="${ep}" />
  <rect x="${-filetGap}" y="${-filetGap}" width="${largeur + filetGap * 2}" height="${hauteur + filetGap * 2}" rx="${r + filetGap}" ry="${r + filetGap}" fill="none" stroke="${p.accent}" stroke-width="${Math.max(0.6, ep * 0.28)}" opacity="0.55" />
  <g stroke="${p.accent}" stroke-width="${Math.max(1, ep * 0.6)}" fill="none" stroke-linecap="square">
    <path d="M ${-cornerGap} ${cornerLen + cornerGap} L ${-cornerGap} ${-cornerGap} L ${cornerLen + cornerGap} ${-cornerGap}" />
    <path d="M ${largeur - cornerLen - cornerGap} ${-cornerGap} L ${largeur + cornerGap} ${-cornerGap} L ${largeur + cornerGap} ${cornerLen + cornerGap}" />
    <path d="M ${-cornerGap} ${hauteur - cornerLen - cornerGap} L ${-cornerGap} ${hauteur + cornerGap} L ${cornerLen + cornerGap} ${hauteur + cornerGap}" />
    <path d="M ${largeur - cornerLen - cornerGap} ${hauteur + cornerGap} L ${largeur + cornerGap} ${hauteur + cornerGap} L ${largeur + cornerGap} ${hauteur - cornerLen - cornerGap}" />
  </g>
  <circle cx="${largeur / 2}" cy="0" r="${fermoirR}" fill="${p.accent}" stroke="${p.principal}" stroke-width="${Math.max(0.5, ep * 0.15)}" />
  ${comet}
</svg>`;
}
