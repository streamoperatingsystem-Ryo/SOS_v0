// Cadre SVG Arena — bordure néon « console rétro » (réf. Xbox 360) :
// octogone allongé, canal sombre sous un tube lumineux double filet,
// médaillons « orb » (anneau + croix X incurvée) sertis sur les 4 biseaux,
// cartouches sombres à cheval (haut décalé droite, bas centré), ailerons
// triangulaires aux milieux des arêtes latérales, nœuds aux 8 sommets.
// Tons dérivés des couleurs utilisateur : couleurFin = néon principal
// (couleur vive unifiée), couleur = profond ; clair = couleur éclaircie.
// Tout est centré sur le tracé du bord ou clippé dans la forme (clipPath) :
// la marge extérieure est rognée par overflow:hidden + clip-path du parent
// (leçon 2026-10-05) et l'intérieur du widget reste libre (règle #6) — les
// orbs/cartouches/ailerons chevauchent le trait, leur moitié externe est
// rognée proprement par le clip.
// opts.animer → nœud voyageur (animateMotion path= direct — pas de mpath,
// compatible vieux CEF OBS) + pulsation SMIL des 4 orbs.

import type { CadreOpts } from "./cadre-cyberpunk";
import { angleVersCoords, assombrir, couleurOu, eclaircir, stopsLiseres } from "./couleurs";

function clampStrokeWidth(sw: number): number {
  const n = Number(sw);
  return Number.isFinite(n) ? Math.max(1, Math.min(20, n)) : 4;
}

export function getArenaBevels(strokeWidth = 4): { bx: number; by: number } {
  const ep = clampStrokeWidth(strokeWidth);
  return {
    bx: Math.max(16, Math.round(ep * 4.2)),
    by: Math.max(10, Math.round(ep * 2.4)),
  };
}

export function getArenaClipPath(
  W: number,
  H: number,
  strokeWidth = 4
): string {
  const { bx, by } = getArenaBevels(strokeWidth);
  const bx2 = Math.min(bx, W / 4);
  const by2 = Math.min(by, H / 4);
  const parts = [
    `M ${bx2} 0`,
    `L ${W - bx2} 0`,
    `L ${W} ${by2}`,
    `L ${W} ${H - by2}`,
    `L ${W - bx2} ${H}`,
    `L ${bx2} ${H}`,
    `L 0 ${H - by2}`,
    `L 0 ${by2}`,
    `Z`,
  ];
  return `path('${parts.join(" ")}')`;
}

export default function renderArena(opts: CadreOpts): string {
  const { largeur, hauteur } = opts;
  const principal = couleurOu(opts.couleurFin, "#ffffff");
  const profond = couleurOu(opts.couleur, assombrir(principal, 0.35));
  const clair = eclaircir(principal, 0.5);
  const angle = Number.isFinite(Number(opts.gradientAngle)) ? Number(opts.gradientAngle) : 135;
  const { x1: gx1, y1: gy1, x2: gx2, y2: gy2 } = angleVersCoords(angle);
  const ep = clampStrokeWidth(opts.strokeWidth ?? 4);
  const offset = ep / 2;
  const { bx, by } = getArenaBevels(ep);
  const bx2 = Math.min(bx, largeur / 4);
  const by2 = Math.min(by, hauteur / 4);

  const margin = Math.max(4, ep);
  const svgW = largeur + offset * 2 + margin * 2;
  const svgH = hauteur + offset * 2 + margin * 2;
  const viewX = -(offset + margin);
  const viewY = -(offset + margin);

  const uid = (opts.idCadre || "d").replace(/[^a-zA-Z0-9_-]/g, "-");
  const gradId = `grad-arena-${uid}`;
  const orbGradId = `orb-grad-arena-${uid}`;
  const clipId = `clip-arena-${uid}`;

  const borderPath = [
    `M ${bx2} 0`,
    `L ${largeur - bx2} 0`,
    `L ${largeur} ${by2}`,
    `L ${largeur} ${hauteur - by2}`,
    `L ${largeur - bx2} ${hauteur}`,
    `L ${bx2} ${hauteur}`,
    `L 0 ${hauteur - by2}`,
    `L 0 ${by2}`,
    `Z`,
  ].join(" ");

  // ── Filet intérieur : second octogone parallèle rentrant d'un pas d —
  // le « double trait » du cadre de référence. ──
  const d = Math.max(2.5, ep * 1.1);
  const di = d * 1.4;
  const echoPath = [
    `M ${bx2 + di} ${d}`,
    `L ${largeur - bx2 - di} ${d}`,
    `L ${largeur - d} ${by2 + di}`,
    `L ${largeur - d} ${hauteur - by2 - di}`,
    `L ${largeur - bx2 - di} ${hauteur - d}`,
    `L ${bx2 + di} ${hauteur - d}`,
    `L ${d} ${hauteur - by2 - di}`,
    `L ${d} ${by2 + di}`,
    `Z`,
  ].join(" ");

  // ── Médaillons « orb » aux 4 coins : sphère anneau + croix X incurvée,
  // posés sur le milieu de chaque biseau avec un léger biais vers
  // l'intérieur (normale du biseau × 0.22r) → ~2/3 du médaillon visible,
  // le reste serti dans le bord (clippé par la forme). ──
  const orbR = Math.max(6, Math.min(bx2, by2) * 1.25, ep * 2);
  const orbW = Math.max(1.6, ep * 0.55);
  const bevelLen = Math.hypot(bx2, by2) || 1;
  const orbsCentres: [number, number][] = [
    [bx2 / 2 + (by2 / bevelLen) * orbR * 0.22, by2 / 2 + (bx2 / bevelLen) * orbR * 0.22],
    [largeur - bx2 / 2 - (by2 / bevelLen) * orbR * 0.22, by2 / 2 + (bx2 / bevelLen) * orbR * 0.22],
    [largeur - bx2 / 2 - (by2 / bevelLen) * orbR * 0.22, hauteur - by2 / 2 - (bx2 / bevelLen) * orbR * 0.22],
    [bx2 / 2 + (by2 / bevelLen) * orbR * 0.22, hauteur - by2 / 2 - (bx2 / bevelLen) * orbR * 0.22],
  ];
  const orbs = orbsCentres
    .map(([cx, cy]) => {
      const xr = orbR * 0.6;
      return (
        `<g>` +
        `<circle cx="${cx}" cy="${cy}" r="${orbR}" fill="rgba(6,10,4,0.65)" stroke="${clair}" stroke-width="${orbW}"/>` +
        `<circle cx="${cx}" cy="${cy}" r="${orbR - orbW * 0.9}" fill="url(#${orbGradId})" opacity="0.9"/>` +
        `<path d="M ${cx - xr} ${cy - xr} Q ${cx} ${cy} ${cx + xr} ${cy + xr} M ${cx + xr} ${cy - xr} Q ${cx} ${cy} ${cx - xr} ${cy + xr}" fill="none" stroke="#ffffff" stroke-width="${Math.max(1.4, ep * 0.5)}" stroke-linecap="round" opacity="0.95"/>` +
        `</g>`
      );
    })
    .join("");
  const orbsGroupe = opts.animer
    ? `<g>${orbs}<animate attributeName="opacity" values="1;0.55;1" dur="3.5s" repeatCount="indefinite"/></g>`
    : `<g>${orbs}</g>`;

  // ── Cartouches sombres à cheval sur le bord — positions de la
  // référence : haut décalé à droite (« XBOX LIVE »), bas centré
  // (« XBOX 360 »). Hexagone allongé foncé + filet clair + point. ──
  const traitFin = Math.max(1.2, ep * 0.35);
  const cw = Math.max(18, bx2 * 1.15);
  const ch = Math.max(4.5, ep * 1.7);
  const cd = cw * 0.3;
  const dotPt = Math.max(1.4, ep * 0.45);

  function cartouche(cx: number, cy: number, tx: number, ty: number): string {
    const nx = -ty;
    const ny = tx;
    const P = (t: number, n: number) => `${cx + tx * t + nx * n} ${cy + ty * t + ny * n}`;
    const hexa = [
      `M ${P(-cw + cd, -ch)}`,
      `L ${P(cw - cd, -ch)}`,
      `L ${P(cw, 0)}`,
      `L ${P(cw - cd, ch)}`,
      `L ${P(-cw + cd, ch)}`,
      `L ${P(-cw, 0)}`,
      `Z`,
    ].join(" ");
    const barre = `M ${P(-cw * 0.5, ch * 0.35)} L ${P(cw * 0.5, ch * 0.35)}`;
    const ptG = P(-cw + cd * 1.2, ch * 0.35).split(" ");
    const ptD = P(cw - cd * 1.2, ch * 0.35).split(" ");
    return (
      `<path d="${hexa}" fill="rgba(6,10,4,0.75)" stroke="${clair}" stroke-width="${traitFin}"/>` +
      `<path d="${barre}" fill="none" stroke="${clair}" stroke-width="${Math.max(1, traitFin * 0.7)}" opacity="0.6"/>` +
      `<circle cx="${ptG[0]}" cy="${ptG[1]}" r="${dotPt}" fill="${clair}"/>` +
      `<circle cx="${ptD[0]}" cy="${ptD[1]}" r="${dotPt}" fill="${clair}"/>`
    );
  }

  // ── Ailerons triangulaires aux milieux des arêtes latérales —
  // losange à cheval sur le bord → pointe intérieure visible. ──
  const wingL = Math.max(5, ep * 2.2);
  const wingT = Math.max(7, ep * 3);
  function aileron(cx: number, cy: number, nx: number, ny: number): string {
    const tx = -ny;
    const ty = nx;
    const P = (t: number, n: number) => `${cx + tx * t + nx * n} ${cy + ty * t + ny * n}`;
    const kite = [
      `M ${P(-wingT, -wingL * 0.35)}`,
      `L ${P(0, wingL)}`,
      `L ${P(wingT, -wingL * 0.35)}`,
      `L ${P(0, -wingL)}`,
      `Z`,
    ].join(" ");
    return `<path d="${kite}" fill="rgba(6,10,4,0.7)" stroke="${clair}" stroke-width="${traitFin}"/>`;
  }

  // Gardes dimensions : décor seulement si les arêtes sont assez longues.
  const assezLarge = largeur > cw * 4 && hauteur > wingT * 4;
  const decors = assezLarge
    ? cartouche(largeur * 0.72, 0, 1, 0) +
      cartouche(largeur / 2, hauteur, 1, 0) +
      aileron(0, hauteur / 2, 1, 0) +
      aileron(largeur, hauteur / 2, -1, 0)
    : "";

  // ── Nœuds lumineux aux 8 sommets de l'octogone (clippés → demi-nœuds
  // sertis sur les angles). ──
  const sommets = [
    [bx2, 0], [largeur - bx2, 0],
    [largeur, by2], [largeur, hauteur - by2],
    [largeur - bx2, hauteur], [bx2, hauteur],
    [0, hauteur - by2], [0, by2],
  ];
  const noeudR = Math.max(1.3, ep * 0.35);
  const noeuds = sommets
    .map(([nx, ny]) => `<circle cx="${nx}" cy="${ny}" r="${noeudR}" fill="${clair}"/>`)
    .join("");

  // ── Nœud voyageur (UN groupe, UN animateMotion le long du bord —
  // path= direct, pas de mpath : support universel vieux CEF). ──
  const dotR = Math.max(1.8, ep * 0.5);
  const voyageur = opts.animer
    ? `<g>` +
      `<animateMotion dur="5s" repeatCount="indefinite" path="${borderPath}"/>` +
      `<circle r="${dotR * 2.4}" fill="${clair}" opacity="0.35"/>` +
      `<circle r="${dotR}" fill="#ffffff"/>` +
      `</g>`
    : "";

  return `<svg width="${svgW}" height="${svgH}" viewBox="${viewX} ${viewY} ${svgW} ${svgH}" preserveAspectRatio="none" style="position:absolute; top:-${offset + margin}px; left:-${offset + margin}px; pointer-events:none; z-index:5;">
  <defs>
    <linearGradient id="${gradId}" x1="${gx1}%" y1="${gy1}%" x2="${gx2}%" y2="${gy2}%">
      ${stopsLiseres(profond, principal)}
    </linearGradient>
    <radialGradient id="${orbGradId}" cx="35%" cy="30%" r="75%">
      <stop offset="0%" stop-color="${clair}" />
      <stop offset="100%" stop-color="${profond}" />
    </radialGradient>
    <clipPath id="${clipId}">
      <path d="${borderPath}" />
    </clipPath>
  </defs>
  <g clip-path="url(#${clipId})">
    <path d="${borderPath}" fill="none" stroke="rgba(6,10,4,0.55)" stroke-width="${ep * 3.4}" stroke-linejoin="miter" />
    <path d="${borderPath}" fill="none" stroke="${principal}" stroke-width="${ep * 2.6}" stroke-linejoin="miter" opacity="0.28" />
    <path d="${borderPath}" fill="none" stroke="url(#${gradId})" stroke-width="${ep}" stroke-linejoin="miter" />
    <path d="${borderPath}" fill="none" stroke="${clair}" stroke-width="${Math.max(0.8, ep * 0.2)}" stroke-linejoin="miter" opacity="0.5" />
    <path d="${echoPath}" fill="none" stroke="${clair}" stroke-width="${Math.max(1, ep * 0.28)}" stroke-linejoin="miter" opacity="0.75" />
    ${noeuds}
    ${decors}
    ${orbsGroupe}
  </g>
  ${voyageur}
</svg>`;
}
