// Cadre SVG Hexagone — bordure hexagonale avec alvéoles aux coins.
// Variantes : ambre, azure, emerald, violet, steel.

import type { CadreOpts } from "./cadre-cyberpunk";

interface Variante {
  principal: string;
  profond: string;
  clair: string;
}

const VARIANTES: Record<string, Variante> = {
  ambre: { principal: "#f59e0b", profond: "#b45309", clair: "#fcd34d" },
  azure: { principal: "#0ea5e9", profond: "#0369a1", clair: "#7dd3fc" },
  emerald: { principal: "#10b981", profond: "#047857", clair: "#6ee7b7" },
  violet: { principal: "#8b5cf6", profond: "#6d28d9", clair: "#c4b5fd" },
  steel: { principal: "#64748b", profond: "#334155", clair: "#cbd5e1" },
};

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
  strokeWidth = 4,
  _variante = "ambre"
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
  const p = VARIANTES[opts.variante ?? "ambre"] ?? VARIANTES.ambre;
  const ep = clampStrokeWidth(opts.strokeWidth ?? 4);
  const s = Math.min(getHexagonClipSize(ep), largeur / 4, hauteur / 4);

  const margin = Math.max(10, ep * 2.5);
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

  const hexR = Math.max(5, ep * 1.4);
  const corners = [
    { cx: s * 0.5, cy: s * 0.35 },
    { cx: largeur - s * 0.5, cy: s * 0.35 },
    { cx: s * 0.5, cy: hauteur - s * 0.35 },
    { cx: largeur - s * 0.5, cy: hauteur - s * 0.35 },
  ];

  const cornerHexes = corners
    .map((c) => `<path d="${hexPath(c.cx, c.cy, hexR)}" fill="none" stroke="${p.clair}" stroke-width="${Math.max(1, ep * 0.35)}"/>`)
    .join("");

  return `<svg width="${svgW}" height="${svgH}" viewBox="${vx} ${vy} ${svgW} ${svgH}" preserveAspectRatio="none" style="position:absolute; top:-${margin}px; left:-${margin}px; pointer-events:none; z-index:5;">
  <defs>
    <linearGradient id="${gradId}" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="${p.principal}" />
      <stop offset="100%" stop-color="${p.profond}" />
    </linearGradient>
    <clipPath id="${clipId}">
      <path d="${borderPath}" />
    </clipPath>
  </defs>
  <g clip-path="url(#${clipId})">
    <path d="${borderPath}" fill="none" stroke="${p.principal}" stroke-width="${ep * 2}" stroke-linejoin="miter" opacity="0.2" />
    <path d="${borderPath}" fill="none" stroke="url(#${gradId})" stroke-width="${ep}" stroke-linejoin="miter" />
    <path d="${borderPath}" fill="none" stroke="${p.clair}" stroke-width="${Math.max(1, ep * 0.3)}" stroke-linejoin="miter" opacity="0.6" />
  </g>
  <g opacity="0.7">
    ${cornerHexes}
    <path d="${hexPath(largeur / 2, 0, hexR * 0.7)}" fill="none" stroke="${p.clair}" stroke-width="${Math.max(1, ep * 0.3)}" opacity="0.5" />
    <path d="${hexPath(largeur / 2, hauteur, hexR * 0.7)}" fill="none" stroke="${p.clair}" stroke-width="${Math.max(1, ep * 0.3)}" opacity="0.5" />
  </g>
  <path d="${borderPath}" fill="none" stroke="${p.clair}" stroke-width="${Math.max(1, ep * 0.2)}" stroke-linejoin="miter" opacity="0.4" transform="translate(${ep * 0.6} ${ep * 0.6}) scale(${Math.max(0.01, 1 - (ep * 1.2) / largeur)} ${Math.max(0.01, 1 - (ep * 1.2) / hauteur)})" />
</svg>`;
}
