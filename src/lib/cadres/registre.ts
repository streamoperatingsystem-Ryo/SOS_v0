// Registre central des cadres SVG — 7 cadres disponibles.
// Tous les cadres sont personnalisables : rendus avec dégradé
// couleur/couleurFin + angle de gradient utilisateur. Les presets
// (cyberpunk, story, thin-line, hexagon) dérivent leurs tons internes
// de ces couleurs (voir cadre-*.ts) — plus de palettes/variantes figées.

import type { CadreOpts } from "./cadre-cyberpunk";
import renderCyberpunk, { getCyberpunkClipPath } from "./cadre-cyberpunk";
import renderStory, { getStoryClipPath } from "./cadre-story";
import renderThinLine, { getThinLineClipPath } from "./cadre-thin-line";
import renderHexagon, { getHexagonClipPath } from "./cadre-hexagon";
import renderMGStyle, { getMGStyleClipPath } from "./cadre-mgstyle";
import renderArena, { getArenaClipPath } from "./cadre-arena";
import renderTribal, { getTribalClipPath } from "./cadre-tribal";
import { angleVersCoords } from "./couleurs";

export interface CadreRegistre {
  id: string;
  nom: string;
  categorie: string;
}

export const REGISTRE_CADRES: CadreRegistre[] = [
  { id: "arrondis", nom: "Arrondis", categorie: "minimaliste" },
  { id: "carre", nom: "Carré", categorie: "minimaliste" },
  { id: "mgstyle", nom: "MGStyle", categorie: "tactique" },
  { id: "cyberpunk", nom: "Cyberpunk", categorie: "neon" },
  { id: "story", nom: "Story", categorie: "romantique" },
  { id: "thin-line", nom: "Thin Line", categorie: "minimaliste" },
  { id: "hexagon", nom: "Hexagone", categorie: "geometrique" },
  { id: "arena", nom: "Arena", categorie: "gaming" },
  { id: "tribal", nom: "Tribal", categorie: "sombre" },
];

type RendererFn = (opts: CadreOpts) => string;
type ClipPathFn = (W: number, H: number, strokeWidth: number) => string;

const CADRES_MAP: Record<string, RendererFn> = {
  cyberpunk: renderCyberpunk,
  story: renderStory,
  "thin-line": renderThinLine,
  hexagon: renderHexagon,
  mgstyle: renderMGStyle,
  arena: renderArena,
  tribal: renderTribal,
};

const MAP_CLIP_PATHS: Record<string, ClipPathFn> = {
  cyberpunk: getCyberpunkClipPath,
  story: getStoryClipPath,
  "thin-line": getThinLineClipPath,
  hexagon: getHexagonClipPath,
  mgstyle: getMGStyleClipPath,
  arena: getArenaClipPath,
  tribal: getTribalClipPath,
};

/// Génère le SVG d'un cadre selon son style. Tous les cadres sont rendus
/// avec le dégradé couleur/couleurFin + angle utilisateur ; les presets
/// dérivent leurs tons internes de ces couleurs.
export function genererSVG(
  style: string,
  largeur: number,
  hauteur: number,
  couleur: string,
  couleurFin: string,
  strokeWidth: number,
  idCadre: string | undefined,
  gradientAngle: number | undefined,
  animer?: boolean
): string {
  const c = /^#[0-9a-f]{6}$/i.test(couleur || "") ? couleur : "#ffffff";
  const cFin = /^#[0-9a-f]{6}$/i.test(couleurFin || "") ? couleurFin : c;
  const ep = Number.isFinite(Number(strokeWidth)) ? Math.max(1, Number(strokeWidth)) : 4;
  const offset = ep / 2;
  const angle = Number.isFinite(Number(gradientAngle)) ? Number(gradientAngle) : 135;
  const { x1, y1, x2, y2 } = angleVersCoords(angle);

  // ── Cadres inline (arrondis, carre) ────────────────────────────────────
  if (style === "arrondis" || style === "carre") {
    const rx = style === "arrondis" ? 8 : 0;
    const gradId = `grad-cadre-${(idCadre || "d").replace(/[^a-zA-Z0-9_-]/g, "-")}-${style}`;
    // Comète (animer) : segment balayant le tracé du bord — recette
    // thin-line (pathLength=100 + dashoffset SMIL). SUR le bord : la
    // marge est rognée par overflow:hidden/clip-path du parent → visible
    // sur widgets ET bord du canvas.
    const rectPath = rx > 0
      ? `M ${rx} 0 L ${largeur - rx} 0 Q ${largeur} 0 ${largeur} ${rx} L ${largeur} ${hauteur - rx} Q ${largeur} ${hauteur} ${largeur - rx} ${hauteur} L ${rx} ${hauteur} Q 0 ${hauteur} 0 ${hauteur - rx} L 0 ${rx} Q 0 0 ${rx} 0 Z`
      : `M 0 0 L ${largeur} 0 L ${largeur} ${hauteur} L 0 ${hauteur} Z`;
    const comet = animer === true
      ? `<path d="${rectPath}" fill="none" stroke="${cFin}" stroke-width="${Math.max(1.2, ep * 0.7)}" stroke-linecap="round" pathLength="100" stroke-dasharray="7 93" stroke-dashoffset="0">` +
        `<animate attributeName="stroke-dashoffset" from="0" to="-100" dur="4s" repeatCount="indefinite"/>` +
        `</path>`
      : "";
    return `<svg width="${largeur + offset * 2}" height="${hauteur + offset * 2}" viewBox="-${offset} -${offset} ${largeur + offset * 2} ${hauteur + offset * 2}" preserveAspectRatio="none" style="position:absolute;top:-${offset}px;left:-${offset}px;pointer-events:none;z-index:5;">
        <defs>
          <linearGradient id="${gradId}" x1="${x1}%" y1="${y1}%" x2="${x2}%" y2="${y2}%">
            <stop offset="0%" stop-color="${c}" stop-opacity="1"/>
            <stop offset="100%" stop-color="${cFin}" stop-opacity="1"/>
          </linearGradient>
        </defs>
        <rect x="0" y="0" width="${largeur}" height="${hauteur}" rx="${rx}" ry="${rx}" fill="none" stroke="url(#${gradId})" stroke-width="${ep}"/>
        ${comet}
      </svg>`;
  }

  // ── Cadres avancés : dispatch ───────────────────────────────────────────
  const renderer = CADRES_MAP[style];
  if (renderer) {
    return renderer({ largeur, hauteur, idCadre, strokeWidth: ep, couleur: c, couleurFin: cFin, gradientAngle: angle, animer: animer === true });
  }

  return "";
}

/// Génère une chaîne CSS clip-path: path('...') pour découper le contenu
/// du widget selon la forme du cadre. Retourne null si le cadre a une bordure
/// rectangulaire simple (pas de clip supplémentaire nécessaire).
export function genererClipPathCadre(
  style: string,
  W: number,
  H: number,
  strokeWidth: number
): string | null {
  if (!style) return null;

  if (style === "arrondis") {
    const r = Math.min(8, W / 4, H / 4);
    return `path('M ${r} 0 L ${W - r} 0 Q ${W} 0 ${W} ${r} L ${W} ${H - r} Q ${W} ${H} ${W - r} ${H} L ${r} ${H} Q 0 ${H} 0 ${H - r} L 0 ${r} Q 0 0 ${r} 0 Z')`;
  }
  if (style === "carre") return null;

  const fn = MAP_CLIP_PATHS[style];
  if (!fn) return null;
  try {
    return fn(W, H, strokeWidth);
  } catch {
    return null;
  }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

export function trouverCadre(id: string): CadreRegistre | undefined {
  return REGISTRE_CADRES.find((c) => c.id === id);
}

/// Dégradé de couleur d'un cadre pour le rendu du titre de widget.
/// Retourne { from, to } :
/// - Cadre actif → couleur/couleurFin du user.
/// - Pas de cadre actif (cadre null/inactif) → dégradé blanc→gris par défaut.
export function gradientCadre(
  cadre: { couleur: string; couleurFin: string; actif: boolean } | null | undefined
): { from: string; to: string } {
  if (!cadre || !cadre.actif) {
    return { from: "#e0e0e0", to: "#9a9a9a" };
  }
  return { from: cadre.couleur || "#ffffff", to: cadre.couleurFin || cadre.couleur || "#000000" };
}
