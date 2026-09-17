// Cadre SVG Cyberpunk — bordure néon avec coins biseautés, plaques HUD,
// circuits et diodes. Variantes : defaut, vert-magenta, orange-cyan,
// violet-cyan, rouge-cyan.

interface Variante {
  c1: string; // couleur néon principale
  c2: string; // couleur néon secondaire
  c3: string; // blanc highlight
}

const VARIANTES: Record<string, Variante> = {
  defaut: { c1: "#00f0ff", c2: "#ff00ff", c3: "#ffffff" },
  "vert-magenta": { c1: "#39ff14", c2: "#ff00ff", c3: "#ffffff" },
  "orange-cyan": { c1: "#ff8800", c2: "#00f0ff", c3: "#ffffff" },
  "violet-cyan": { c1: "#8c4dff", c2: "#00f0ff", c3: "#ffffff" },
  "rouge-cyan": { c1: "#ff2244", c2: "#00f0ff", c3: "#ffffff" },
};

export interface CadreOpts {
  largeur: number;
  hauteur: number;
  idCadre?: string;
  strokeWidth?: number;
  variante?: string;
  couleur?: string;
  couleurFin?: string;
  gradientAngle?: number;
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
  strokeWidth = 4,
  _variante?: string
): string {
  const s = Math.min(getCyberpunkBevelSize(strokeWidth), W / 4, H / 4);
  return `path('M ${s} 0 L ${W - s} 0 L ${W} ${s} L ${W} ${H - s} L ${W - s} ${H} L ${s} ${H} L 0 ${H - s} L 0 ${s} Z')`;
}

export default function renderCyberpunk(opts: CadreOpts): string {
  const { largeur, hauteur } = opts;
  const pal = VARIANTES[opts.variante ?? "defaut"] ?? VARIANTES.defaut;
  const { c1, c2, c3 } = pal;
  const ep = clampStrokeWidth(opts.strokeWidth ?? 4);
  const offset = ep / 2;
  const bevel = Math.max(8, Math.round(ep * 3.5));

  const margin = Math.max(4, ep);
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
    const dx = X(bevel * 0.62), dy = Y(bevel * 0.62);
    const dr = Math.max(1.4, ep * 0.4);
    return (
      `<g>` +
      `<path d="${plaque}" fill="${c2}" fill-opacity="0.28" stroke="${c1}" stroke-width="${traitFin}" stroke-linejoin="miter"/>` +
      `<path d="${chevron}" fill="none" stroke="${c2}" stroke-width="${traitFin}" opacity="0.85"/>` +
      `<circle cx="${dx}" cy="${dy}" r="${dr}" fill="${c3}" stroke="${c1}" stroke-width="${Math.max(0.6, ep * 0.12)}"/>` +
      `</g>`
    );
  }

  const brackets =
    bracketCoin(x0, y0, 1, 1) +
    bracketCoin(x1, y0, -1, 1) +
    bracketCoin(x1, y1, -1, -1) +
    bracketCoin(x0, y1, 1, -1);

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

  return `<svg width="${svgW}" height="${svgH}" viewBox="${viewX} ${viewY} ${svgW} ${svgH}" preserveAspectRatio="none" style="position:absolute; top:-${offset + margin}px; left:-${offset + margin}px; pointer-events:none; z-index:5;">
  <defs>
    <clipPath id="${clipPathId}">
      <path d="${pathBorder}" />
    </clipPath>
    <linearGradient id="${gradId}" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="${c1}" />
      <stop offset="50%" stop-color="${c2}" />
      <stop offset="100%" stop-color="${c1}" />
    </linearGradient>
    <linearGradient id="${gradInnerId}" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="${c3}" stop-opacity="0.9" />
      <stop offset="50%" stop-color="${c1}" stop-opacity="0.6" />
      <stop offset="100%" stop-color="${c3}" stop-opacity="0.9" />
    </linearGradient>
  </defs>
  <g clip-path="url(#${clipPathId})">
    <path d="${pathBorder}" fill="none" stroke="${c1}" stroke-width="${ep * 2.2}" stroke-linejoin="miter" opacity="0.25" />
    <path d="${pathBorder}" fill="none" stroke="url(#${gradId})" stroke-width="${ep}" stroke-linejoin="miter" />
    <path d="${pathBorder}" fill="none" stroke="url(#${gradInnerId})" stroke-width="${Math.max(1, ep * 0.35)}" stroke-linejoin="miter" opacity="0.8" />
    <path d="${pathBorder}" fill="none" stroke="${c3}" stroke-width="${Math.max(0.8, ep * 0.2)}" stroke-linejoin="miter" opacity="0.4" />
  </g>
  <g>
    ${circuits}
  </g>
  ${brackets}
</svg>`;
}
