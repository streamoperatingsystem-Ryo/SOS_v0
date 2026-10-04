// Cadre SVG Thin Line — bordure fine minimaliste avec accents aux coins.
// Tons dérivés des couleurs utilisateur : couleur = trait principal,
// couleurFin = accent (défaut : couleur éclaircie).

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

  const margin = Math.max(4, ep);
  const svgW = largeur + margin * 2;
  const svgH = hauteur + margin * 2;
  const vx = -margin;
  const vy = -margin;

  const cornerLen = Math.max(10, ep * 3);
  const cornerGap = Math.max(3, ep * 0.8);

  return `<svg width="${svgW}" height="${svgH}" viewBox="${vx} ${vy} ${svgW} ${svgH}" preserveAspectRatio="none" style="position:absolute; top:-${margin}px; left:-${margin}px; pointer-events:none; z-index:5;">
  <rect x="0" y="0" width="${largeur}" height="${hauteur}" rx="${r}" ry="${r}" fill="none" stroke="${p.principal}" stroke-width="${ep}" />
  <g stroke="${p.accent}" stroke-width="${Math.max(1, ep * 0.6)}" fill="none" stroke-linecap="square">
    <path d="M ${-cornerGap} ${cornerLen + cornerGap} L ${-cornerGap} ${-cornerGap} L ${cornerLen + cornerGap} ${-cornerGap}" />
    <path d="M ${largeur - cornerLen - cornerGap} ${-cornerGap} L ${largeur + cornerGap} ${-cornerGap} L ${largeur + cornerGap} ${cornerLen + cornerGap}" />
    <path d="M ${-cornerGap} ${hauteur - cornerLen - cornerGap} L ${-cornerGap} ${hauteur + cornerGap} L ${cornerLen + cornerGap} ${hauteur + cornerGap}" />
    <path d="M ${largeur - cornerLen - cornerGap} ${hauteur + cornerGap} L ${largeur + cornerGap} ${hauteur + cornerGap} L ${largeur + cornerGap} ${hauteur - cornerLen - cornerGap}" />
  </g>
  <g fill="${p.accent}">
    <circle cx="${largeur / 2}" cy="0" r="${Math.max(2, ep * 0.5)}" />
    <circle cx="${largeur / 2}" cy="${hauteur}" r="${Math.max(2, ep * 0.5)}" />
    <circle cx="0" cy="${hauteur / 2}" r="${Math.max(2, ep * 0.5)}" />
    <circle cx="${largeur}" cy="${hauteur / 2}" r="${Math.max(2, ep * 0.5)}" />
  </g>
</svg>`;
}
