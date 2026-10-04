// Cadre SVG Story — bordure dégradée avec coins en coeur, coeurs décoratifs.
// Tons dérivés des couleurs utilisateur : couleur = vif, couleurFin =
// profond ; clair/pale = couleur éclaircie (aucune palette figée).

import type { CadreOpts } from "./cadre-cyberpunk";
import { angleVersCoords, assombrir, couleurOu, eclaircir } from "./couleurs";

function clampStrokeWidth(sw: number): number {
  const n = Number(sw);
  return Number.isFinite(n) ? Math.max(1, Math.min(20, n)) : 4;
}

export function getStoryClipSize(strokeWidth = 4): number {
  return Math.max(12, Math.round(clampStrokeWidth(strokeWidth) * 4));
}

export function getStoryClipPath(
  W: number,
  H: number,
  strokeWidth = 4
): string {
  const s = Math.min(getStoryClipSize(strokeWidth), W / 4, H / 4);
  const parts = [
    `M ${s} 0`,
    `L ${W - s} 0`,
    `C ${W - s * 0.65} ${-s * 0.12} ${W - s * 0.12} ${-s * 0.12} ${W - s * 0.4} ${s * 0.08}`,
    `C ${W - s * 0.68} ${s * 0.28} ${W + s * 0.12} ${s * 0.65} ${W} ${s}`,
    `L ${W} ${H - s}`,
    `C ${W + s * 0.12} ${H - s * 0.65} ${W - s * 0.68} ${H - s * 0.28} ${W - s * 0.4} ${H - s * 0.08}`,
    `C ${W - s * 0.12} ${H + s * 0.12} ${W - s * 0.65} ${H + s * 0.12} ${W - s} ${H}`,
    `L ${s} ${H}`,
    `C ${s * 0.65} ${H + s * 0.12} ${s * 0.12} ${H + s * 0.12} ${s * 0.4} ${H - s * 0.08}`,
    `C ${s * 0.68} ${H - s * 0.28} ${-s * 0.12} ${H - s * 0.65} ${0} ${H - s}`,
    `L ${0} ${s}`,
    `C ${-s * 0.12} ${s * 0.65} ${s * 0.68} ${s * 0.28} ${s * 0.4} ${s * 0.08}`,
    `C ${s * 0.12} ${-s * 0.12} ${s * 0.65} ${-s * 0.12} ${s} ${0}`,
    `Z`,
  ];
  return `path('${parts.join(" ")}')`;
}

function heartShape(cx: number, cy: number, s: number): string {
  return (
    `M ${cx} ${cy + s * 0.25} ` +
    `C ${cx - s * 0.42} ${cy - s * 0.08} ${cx - s * 0.42} ${cy - s * 0.42} ${cx} ${cy - s * 0.18} ` +
    `C ${cx + s * 0.42} ${cy - s * 0.42} ${cx + s * 0.42} ${cy - s * 0.08} ${cx} ${cy + s * 0.25} Z`
  );
}

export default function renderStory(opts: CadreOpts): string {
  const { largeur, hauteur } = opts;
  const vif = couleurOu(opts.couleur, "#ffffff");
  const palette = {
    vif,
    clair: eclaircir(vif, 0.35),
    profond: couleurOu(opts.couleurFin, assombrir(vif, 0.2)),
    pale: eclaircir(vif, 0.75),
  };
  const blanc = "#ffffff";
  const angle = Number.isFinite(Number(opts.gradientAngle)) ? Number(opts.gradientAngle) : 135;
  const { x1: gx1, y1: gy1, x2: gx2, y2: gy2 } = angleVersCoords(angle);

  const ep = clampStrokeWidth(opts.strokeWidth ?? 4);
  const s = Math.min(getStoryClipSize(ep), largeur / 4, hauteur / 4);

  const heartMargin = Math.max(10, ep * 2.5);
  const margin = heartMargin;

  const svgW = largeur + margin * 2;
  const svgH = hauteur + margin * 2;
  const vx = -margin;
  const vy = -margin;

  const uid = (opts.idCadre || "d").replace(/[^a-zA-Z0-9_-]/g, "-");
  const gradId = `grad-story-${uid}`;
  const innerGradId = `grad-inner-story-${uid}`;
  const clipId = `clip-story-${uid}`;

  const x0 = 0, y0 = 0, x1 = largeur, y1 = hauteur;

  const borderPath = [
    `M ${x0 + s} ${y0}`,
    `L ${x1 - s} ${y0}`,
    `C ${x1 - s * 0.65} ${y0 - s * 0.12} ${x1 - s * 0.12} ${y0 - s * 0.12} ${x1 - s * 0.4} ${y0 + s * 0.08}`,
    `C ${x1 - s * 0.68} ${y0 + s * 0.28} ${x1 + s * 0.12} ${y0 + s * 0.65} ${x1} ${y0 + s}`,
    `L ${x1} ${y1 - s}`,
    `C ${x1 + s * 0.12} ${y1 - s * 0.65} ${x1 - s * 0.68} ${y1 - s * 0.28} ${x1 - s * 0.4} ${y1 - s * 0.08}`,
    `C ${x1 - s * 0.12} ${y1 + s * 0.12} ${x1 - s * 0.65} ${y1 + s * 0.12} ${x1 - s} ${y1}`,
    `L ${x0 + s} ${y1}`,
    `C ${x0 + s * 0.65} ${y1 + s * 0.12} ${x0 + s * 0.12} ${y1 + s * 0.12} ${x0 + s * 0.4} ${y1 - s * 0.08}`,
    `C ${x0 + s * 0.68} ${y1 - s * 0.28} ${x0 - s * 0.12} ${y1 - s * 0.65} ${x0} ${y1 - s}`,
    `L ${x0} ${y0 + s}`,
    `C ${x0 - s * 0.12} ${y0 + s * 0.65} ${x0 + s * 0.68} ${y0 + s * 0.28} ${x0 + s * 0.4} ${y0 + s * 0.08}`,
    `C ${x0 + s * 0.12} ${y0 - s * 0.12} ${x0 + s * 0.65} ${y0 - s * 0.12} ${x0 + s} ${y0}`,
    `Z`,
  ].join(" ");

  const cornerHeartSize = Math.max(6, ep * 1.5);
  const smallHeartSize = Math.max(3, ep * 0.7);

  const cornerHearts = [
    { cx: x0 + s * 0.25, cy: y0 + s * 0.25 },
    { cx: x1 - s * 0.25, cy: y0 + s * 0.25 },
    { cx: x0 + s * 0.25, cy: y1 - s * 0.25 },
    { cx: x1 - s * 0.25, cy: y1 - s * 0.25 },
  ];

  const safeStart = s + Math.max(15, ep * 3);
  const safeEndTop = largeur - s - Math.max(15, ep * 3);
  const safeEndSide = hauteur - s - Math.max(15, ep * 3);

  const numTop = Math.max(0, Math.floor((safeEndTop - safeStart) / 70));
  const numSide = Math.max(0, Math.floor((safeEndSide - safeStart) / 70));

  const topHearts = Array.from({ length: numTop }).map((_, i, arr) => ({
    x: safeStart + (arr.length > 0 ? (i + 1) * (safeEndTop - safeStart) / (arr.length + 1) : 0),
    y: y0,
  }));
  const bottomHearts = Array.from({ length: numTop }).map((_, i, arr) => ({
    x: safeStart + (arr.length > 0 ? (i + 1) * (safeEndTop - safeStart) / (arr.length + 1) : 0),
    y: y1,
  }));
  const leftHearts = Array.from({ length: numSide }).map((_, i, arr) => ({
    x: x0,
    y: safeStart + (arr.length > 0 ? (i + 1) * (safeEndSide - safeStart) / (arr.length + 1) : 0),
  }));
  const rightHearts = Array.from({ length: numSide }).map((_, i, arr) => ({
    x: x1,
    y: safeStart + (arr.length > 0 ? (i + 1) * (safeEndSide - safeStart) / (arr.length + 1) : 0),
  }));

  const cornerHeartsSvg = cornerHearts
    .map((h) => `<path d="${heartShape(h.cx, h.cy, cornerHeartSize)}" fill="${blanc}" opacity="0.92"/>`)
    .join("");
  const smallHeartsSvg = [
    ...topHearts.map((h) => `<path d="${heartShape(h.x, h.y, smallHeartSize)}" fill="${blanc}"/>`),
    ...bottomHearts.map((h) => `<path d="${heartShape(h.x, h.y, smallHeartSize)}" fill="${blanc}"/>`),
    ...leftHearts.map((h) => `<path d="${heartShape(h.x, h.y, smallHeartSize)}" fill="${blanc}"/>`),
    ...rightHearts.map((h) => `<path d="${heartShape(h.x, h.y, smallHeartSize)}" fill="${blanc}"/>`),
  ].join("");

  return `<svg width="${svgW}" height="${svgH}" viewBox="${vx} ${vy} ${svgW} ${svgH}" preserveAspectRatio="none" style="position:absolute; top:-${margin}px; left:-${margin}px; pointer-events:none; z-index:5;">
  <defs>
    <linearGradient id="${gradId}" x1="${gx1}%" y1="${gy1}%" x2="${gx2}%" y2="${gy2}%">
      <stop offset="0%" stop-color="${palette.vif}" />
      <stop offset="50%" stop-color="${palette.clair}" />
      <stop offset="100%" stop-color="${palette.profond}" />
    </linearGradient>
    <linearGradient id="${innerGradId}" x1="${gx1}%" y1="${gy1}%" x2="${gx2}%" y2="${gy2}%">
      <stop offset="0%" stop-color="${blanc}" stop-opacity="0.7" />
      <stop offset="50%" stop-color="${palette.pale}" stop-opacity="0.5" />
      <stop offset="100%" stop-color="${blanc}" stop-opacity="0.7" />
    </linearGradient>
    <clipPath id="${clipId}">
      <path d="${borderPath}" />
    </clipPath>
  </defs>
  <g clip-path="url(#${clipId})">
    <path d="${borderPath}" fill="none" stroke="${palette.vif}" stroke-width="${ep * 2}" stroke-linejoin="round" opacity="0.2" />
    <path d="${borderPath}" fill="none" stroke="url(#${gradId})" stroke-width="${ep}" stroke-linejoin="round" />
    <path d="${borderPath}" fill="none" stroke="url(#${innerGradId})" stroke-width="${Math.max(1, ep * 0.3)}" stroke-linejoin="round" opacity="0.7" />
  </g>
  <g>
    ${cornerHeartsSvg}
  </g>
  <g opacity="0.75">
    ${smallHeartsSvg}
  </g>
</svg>`;
}
