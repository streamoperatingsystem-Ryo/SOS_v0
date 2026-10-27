// Cadre SVG Cyberpunk — bordure néon avec coins biseautés, plaques HUD,
// circuits, code-barres et diodes. Tons dérivés des couleurs utilisateur :
// couleurFin = néon principal (couleur vive unifiée), couleur = néon
// secondaire, highlight blanc.
// opts.animer → un runner lumineux (UN groupe, UN animateMotion) parcourt
// le path du bord — rien dans la zone du widget, marge extérieure seulement.

import { angleVersCoords, assombrir, couleurOu, stopsLiseres } from "./couleurs";

export interface CadreOpts {
  largeur: number;
  hauteur: number;
  idCadre?: string;
  strokeWidth?: number;
  couleur?: string;
  couleurFin?: string;
  gradientAngle?: number;
  /// Émet les éléments SMIL animés du cadre (toggle « Animer » de la modale).
  /// Absent/false = SVG 100% statique, zéro nœud animé.
  animer?: boolean;
}

function clampStrokeWidth(sw: number): number {
  const n = Number(sw);
  return Number.isFinite(n) ? Math.max(1, Math.min(20, n)) : 4;
}

export function getCyberpunkBevelSize(strokeWidth = 4): number {
  return Math.max(8, Math.round(clampStrokeWidth(strokeWidth) * 3.5));
}

export function getCyberpunkClipPath(
  W: number,
  H: number,
  strokeWidth = 4
): string {
  const s = Math.min(getCyberpunkBevelSize(strokeWidth), W / 4, H / 4);
  return `path('M ${s} 0 L ${W - s} 0 L ${W} ${s} L ${W} ${H - s} L ${W - s} ${H} L ${s} ${H} L 0 ${H - s} L 0 ${s} Z')`;
}

export default function renderCyberpunk(opts: CadreOpts): string {
  const { largeur, hauteur } = opts;
  const c1 = couleurOu(opts.couleurFin, "#ffffff");
  const c2 = couleurOu(opts.couleur, assombrir(c1, 0.35));
  const c3 = "#ffffff";
  const angle = Number.isFinite(Number(opts.gradientAngle)) ? Number(opts.gradientAngle) : 135;
  const { x1: gx1, y1: gy1, x2: gx2, y2: gy2 } = angleVersCoords(angle);
  const ep = clampStrokeWidth(opts.strokeWidth ?? 4);
  const offset = ep / 2;
  const bevel = Math.max(8, Math.round(ep * 3.5));

  const margin = Math.max(6, ep * 1.4);
  const svgW = largeur + offset * 2 + margin * 2;
  const svgH = hauteur + offset * 2 + margin * 2;
  const viewX = -(offset + margin);
  const viewY = -(offset + margin);

  const uid = (opts.idCadre || "d").replace(/[^a-zA-Z0-9_-]/g, "-");
  const gradId = `grad-cyberpunk-${uid}`;
  const gradInnerId = `grad-inner-${uid}`;
  const clipPathId = `clip-cyberpunk-${uid}`;

  const x0 = 0, y0 = 0, x1 = largeur, y1 = hauteur;

  const pathBorder = [
    `M ${x0 + bevel} ${y0}`,
    `L ${x1 - bevel} ${y0}`,
    `L ${x1} ${y0 + bevel}`,
    `L ${x1} ${y1 - bevel}`,
    `L ${x1 - bevel} ${y1}`,
    `L ${x0 + bevel} ${y1}`,
    `L ${x0} ${y1 - bevel}`,
    `L ${x0} ${y0 + bevel}`,
    "Z",
  ].join(" ");

  const d = Math.max(2, ep * 0.8);
  const b = Math.min(bevel * 1.6, largeur / 5, hauteur / 5);
  const traitFin = Math.max(0.8, ep * 0.22);

  // ── Plaques HUD aux 4 coins (marge extérieure) : plaque biseautée +
  // chevron + diode. ──
  function bracketCoin(cx: number, cy: number, sx: number, sy: number): string {
    const X = (x: number) => cx + x * sx;
    const Y = (y: number) => cy + y * sy;
    const plaque = [
      `M ${X(b + d)} ${Y(-d)}`,
      `L ${X(bevel * 0.55)} ${Y(-d)}`,
      `L ${X(-d)} ${Y(bevel * 0.55)}`,
      `L ${X(-d)} ${Y(b + d)}`,
      `L ${X(-d * 2.2)} ${Y(b + d)}`,
      `L ${X(-d * 2.2)} ${Y(bevel * 0.35)}`,
      `L ${X(bevel * 0.35)} ${Y(-d * 2.2)}`,
      `L ${X(b + d)} ${Y(-d * 2.2)}`,
      "Z",
    ].join(" ");
    const chevron = [
      `M ${X(bevel * 0.9)} ${Y(-d * 3.4)}`,
      `L ${X(bevel * 0.28)} ${Y(-d * 3.4)}`,
      `L ${X(-d * 3.4)} ${Y(bevel * 0.28)}`,
      `L ${X(-d * 3.4)} ${Y(bevel * 0.9)}`,
    ].join(" ");
    const chevron2 = [
      `M ${X(bevel * 0.62)} ${Y(-d * 4.6)}`,
      `L ${X(bevel * 0.34)} ${Y(-d * 4.6)}`,
      `L ${X(-d * 4.6)} ${Y(bevel * 0.34)}`,
      `L ${X(-d * 4.6)} ${Y(bevel * 0.62)}`,
    ].join(" ");
    const dx = X(bevel * 0.62), dy = Y(bevel * 0.62);
    const dr = Math.max(1.4, ep * 0.4);
    return (
      `<g>` +
      `<path d="${plaque}" fill="${c2}" fill-opacity="0.28" stroke="${c1}" stroke-width="${traitFin}" stroke-linejoin="miter"/>` +
      `<path d="${chevron}" fill="none" stroke="${c2}" stroke-width="${traitFin}" opacity="0.85"/>` +
      `<path d="${chevron2}" fill="none" stroke="${c2}" stroke-width="${traitFin}" opacity="0.4"/>` +
      `<circle cx="${dx}" cy="${dy}" r="${dr}" fill="${c3}" stroke="${c1}" stroke-width="${Math.max(0.6, ep * 0.12)}"/>` +
      `</g>`
    );
  }

  const brackets =
    bracketCoin(x0, y0, 1, 1) +
    bracketCoin(x1, y0, -1, 1) +
    bracketCoin(x1, y1, -1, -1) +
    bracketCoin(x0, y1, 1, -1);

  // ── Traces circuit sur les 4 bords (marge) ──
  const co = Math.max(2, ep);
  const pr = Math.max(1, ep * 0.28);
  const midX = largeur / 2;
  const midY = hauteur / 2;
  const segH = Math.min(bevel * 1.4, largeur / 8);
  const segV = Math.min(bevel * 1.4, hauteur / 8);
  const circuits = [
    `<path d="M ${midX - segH * 2.2} ${-co} L ${midX - segH * 0.4} ${-co}" stroke="${c1}" stroke-width="${traitFin}" opacity="0.8"/>`,
    `<circle cx="${midX - segH * 2.2}" cy="${-co}" r="${pr}" fill="${c1}"/>`,
    `<path d="M ${midX + segH * 0.4} ${-co} L ${midX + segH * 2.2} ${-co}" stroke="${c1}" stroke-width="${traitFin}" opacity="0.8"/>`,
    `<circle cx="${midX + segH * 2.2}" cy="${-co}" r="${pr}" fill="${c1}"/>`,
    `<path d="M ${midX - segH * 2.2} ${hauteur + co} L ${midX - segH * 0.4} ${hauteur + co}" stroke="${c2}" stroke-width="${traitFin}" opacity="0.8"/>`,
    `<circle cx="${midX - segH * 2.2}" cy="${hauteur + co}" r="${pr}" fill="${c2}"/>`,
    `<path d="M ${midX + segH * 0.4} ${hauteur + co} L ${midX + segH * 2.2} ${hauteur + co}" stroke="${c2}" stroke-width="${traitFin}" opacity="0.8"/>`,
    `<circle cx="${midX + segH * 2.2}" cy="${hauteur + co}" r="${pr}" fill="${c2}"/>`,
    `<path d="M ${-co} ${midY - segV} L ${-co} ${midY + segV}" stroke="${c2}" stroke-width="${traitFin}" opacity="0.8"/>`,
    `<circle cx="${-co}" cy="${midY + segV}" r="${pr}" fill="${c2}"/>`,
    `<path d="M ${largeur + co} ${midY - segV} L ${largeur + co} ${midY + segV}" stroke="${c1}" stroke-width="${traitFin}" opacity="0.8"/>`,
    `<circle cx="${largeur + co}" cy="${midY - segV}" r="${pr}" fill="${c1}"/>`,
  ].join("\n    ");

  // ── Code-barres (marge haut-droite) : micro-data signature cyberpunk ──
  const barH = Math.max(4, ep * 1.1);
  const barY = -margin * 0.55 - barH / 2;
  const barUnit = Math.max(0.8, ep * 0.28);
  const barX0 = x1 - bevel * 0.2 - barUnit * 26;
  const barPattern = [1, 1, 0.5, 2, 0.5, 1, 3, 1, 0.5, 0.5, 2, 1];
  let bx = barX0;
  const barcode = barPattern
    .map((w, i) => {
      const bw = w * barUnit;
      const rect = `<rect x="${bx}" y="${barY}" width="${bw}" height="${barH}" fill="${i % 3 === 0 ? c1 : c2}" opacity="0.85"/>`;
      bx += bw + barUnit;
      return rect;
    })
    .join("");

  // ── Runner lumineux (UN groupe, UN animateMotion le long du bord) ──
  // Émis seulement si animer : halo + cœur dans un <g> animé via mpath.
  const dotR = Math.max(1.8, ep * 0.55);
  const runner = opts.animer
    ? `<g opacity="0.95">` +
      `<animateMotion dur="5s" repeatCount="indefinite" path="${pathBorder}"/>` +
      `<circle r="${dotR * 2.6}" fill="${c1}" opacity="0.35"/>` +
      `<circle r="${dotR}" fill="${c3}"/>` +
      `</g>`
    : "";

  return `<svg width="${svgW}" height="${svgH}" viewBox="${viewX} ${viewY} ${svgW} ${svgH}" preserveAspectRatio="none" style="position:absolute; top:-${offset + margin}px; left:-${offset + margin}px; pointer-events:none; z-index:5;">
  <defs>
    <clipPath id="${clipPathId}">
      <path d="${pathBorder}" />
    </clipPath>
    <linearGradient id="${gradId}" x1="${gx1}%" y1="${gy1}%" x2="${gx2}%" y2="${gy2}%">
      ${stopsLiseres(c2, c1)}
    </linearGradient>
    <linearGradient id="${gradInnerId}" x1="${gx1}%" y1="${gy1}%" x2="${gx2}%" y2="${gy2}%">
      <stop offset="0%" stop-color="${c3}" stop-opacity="0.9" />
      <stop offset="50%" stop-color="${c1}" stop-opacity="0.6" />
      <stop offset="100%" stop-color="${c3}" stop-opacity="0.9" />
    </linearGradient>
  </defs>
  <g clip-path="url(#${clipPathId})">
    <path d="${pathBorder}" fill="none" stroke="${c1}" stroke-width="${ep * 2.6}" stroke-linejoin="miter" opacity="0.22" />
    <path d="${pathBorder}" fill="none" stroke="url(#${gradId})" stroke-width="${ep}" stroke-linejoin="miter" />
    <path d="${pathBorder}" fill="none" stroke="url(#${gradInnerId})" stroke-width="${Math.max(1, ep * 0.35)}" stroke-linejoin="miter" opacity="0.85" />
    <path d="${pathBorder}" fill="none" stroke="${c3}" stroke-width="${Math.max(0.8, ep * 0.2)}" stroke-linejoin="miter" opacity="0.4" />
  </g>
  <g>
    ${circuits}
  </g>
  ${brackets}
  ${barcode}
  ${runner}
</svg>`;
}
