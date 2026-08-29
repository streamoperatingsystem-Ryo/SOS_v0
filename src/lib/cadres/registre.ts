// Registre central des cadres SVG — 6 cadres disponibles.
// Personnalisables (arrondis, carre) : rendus inline avec dégradé.
// Presets (cyberpunk, story, thin-line, hexagon) : renderer dédié.

import type { CadreOpts } from "./cadre-cyberpunk";
import renderCyberpunk, { getCyberpunkClipPath } from "./cadre-cyberpunk";
import renderStory, { getStoryClipPath } from "./cadre-story";
import renderThinLine, { getThinLineClipPath } from "./cadre-thin-line";
import renderHexagon, { getHexagonClipPath } from "./cadre-hexagon";

export interface Variante {
  id: string;
  nom: string;
}

export interface CadreRegistre {
  id: string;
  nom: string;
  categorie: string;
  variantes: Variante[] | null;
  personnalisable: boolean;
}

export const REGISTRE_CADRES: CadreRegistre[] = [
  // ── Cadres personnalisables (couleur utilisateur) ──────────────────────────
  { id: "arrondis", nom: "Arrondis", categorie: "minimaliste", variantes: null, personnalisable: true },
  { id: "carre", nom: "Carré", categorie: "minimaliste", variantes: null, personnalisable: true },

  // ── Presets statiques ──────────────────────────────────────────────────────
  {
    id: "cyberpunk",
    nom: "Cyberpunk",
    categorie: "neon",
    variantes: [
      { id: "defaut", nom: "Cyan/Magenta" },
      { id: "vert-magenta", nom: "Vert/Magenta" },
      { id: "orange-cyan", nom: "Orange/Cyan" },
      { id: "violet-cyan", nom: "Violet/Cyan" },
      { id: "rouge-cyan", nom: "Rouge/Cyan" },
    ],
    personnalisable: false,
  },
  {
    id: "story",
    nom: "Story",
    categorie: "romantique",
    variantes: [
      { id: "pink", nom: "Rose" },
      { id: "blue", nom: "Bleu" },
      { id: "purple", nom: "Violet" },
      { id: "green", nom: "Vert" },
      { id: "orange", nom: "Orange" },
      { id: "yellow", nom: "Jaune" },
    ],
    personnalisable: false,
  },
  {
    id: "thin-line",
    nom: "Thin Line",
    categorie: "minimaliste",
    variantes: [
      { id: "graphite", nom: "Graphite" },
      { id: "argent", nom: "Argent" },
      { id: "blanc-pure", nom: "Blanc Pur" },
    ],
    personnalisable: false,
  },
  {
    id: "hexagon",
    nom: "Hexagone",
    categorie: "geometrique",
    variantes: [
      { id: "ambre", nom: "Ambre" },
      { id: "azure", nom: "Azure" },
      { id: "emerald", nom: "Émeraude" },
      { id: "violet", nom: "Violet" },
      { id: "steel", nom: "Acier" },
    ],
    personnalisable: false,
  },
];

type RendererFn = (opts: CadreOpts) => string;
type ClipPathFn = (W: number, H: number, strokeWidth: number, variante?: string) => string;

const CADRES_MAP: Record<string, RendererFn> = {
  cyberpunk: renderCyberpunk,
  story: renderStory,
  "thin-line": renderThinLine,
  hexagon: renderHexagon,
};

const MAP_CLIP_PATHS: Record<string, ClipPathFn> = {
  cyberpunk: getCyberpunkClipPath,
  story: getStoryClipPath,
  "thin-line": getThinLineClipPath,
  hexagon: getHexagonClipPath,
};

/// Génère le SVG d'un cadre selon son style. Les cadres personnalisables
/// (arrondis, carre) sont rendus inline avec dégradé couleur/couleurFin.
/// Les presets sont dispatchés vers leur renderer dédié.
export function genererSVG(
  style: string,
  variante: string | undefined,
  largeur: number,
  hauteur: number,
  couleur: string,
  couleurFin: string,
  strokeWidth: number,
  idCadre: string | undefined
): string {
  const c = /^#[0-9a-f]{6}$/i.test(couleur || "") ? couleur : "#ffffff";
  const cFin = /^#[0-9a-f]{6}$/i.test(couleurFin || "") ? couleurFin : c;
  const ep = Number.isFinite(Number(strokeWidth)) ? Math.max(1, Number(strokeWidth)) : 4;
  const offset = ep / 2;

  // ── Cadres personnalisables (inline) ──────────────────────────────────────
  if (style === "arrondis" || style === "carre") {
    const rx = style === "arrondis" ? 8 : 0;
    const gradId = `grad-cadre-${(idCadre || "d").replace(/[^a-zA-Z0-9_-]/g, "-")}-${style}`;
    return `<svg width="${largeur + offset * 2}" height="${hauteur + offset * 2}" viewBox="-${offset} -${offset} ${largeur + offset * 2} ${hauteur + offset * 2}" preserveAspectRatio="none" style="position:absolute;top:-${offset}px;left:-${offset}px;pointer-events:none;z-index:5;">
        <defs>
          <linearGradient id="${gradId}" x1="0%" y1="0%" x2="100%" y2="100%">
            <stop offset="0%" stop-color="${c}" stop-opacity="1"/>
            <stop offset="100%" stop-color="${cFin}" stop-opacity="1"/>
          </linearGradient>
        </defs>
        <rect x="0" y="0" width="${largeur}" height="${hauteur}" rx="${rx}" ry="${rx}" fill="none" stroke="url(#${gradId})" stroke-width="${ep}"/>
      </svg>`;
  }

  // ── Presets : dispatch ─────────────────────────────────────────────────────
  const renderer = CADRES_MAP[style];
  if (renderer) {
    return renderer({ largeur, hauteur, idCadre, strokeWidth: ep, variante, couleur: c, couleurFin: cFin });
  }

  return "";
}

/// Génère une chaîne CSS clip-path: path('...') pour découper le contenu
/// du widget selon la forme du cadre. Retourne null si le cadre a une bordure
/// rectangulaire simple (pas de clip supplémentaire nécessaire).
export function genererClipPathCadre(
  style: string,
  variante: string | undefined,
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
    return fn(W, H, strokeWidth, variante);
  } catch {
    return null;
  }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

export function compterCadres(): number {
  return REGISTRE_CADRES.reduce((total, c) => {
    if (c.variantes && c.variantes.length > 0) return total + c.variantes.length;
    return total + 1;
  }, 0);
}

export function getCategories(): string[] {
  const cats = [...new Set(REGISTRE_CADRES.map((c) => c.categorie))];
  return cats.sort();
}

export function trouverCadre(id: string): CadreRegistre | undefined {
  return REGISTRE_CADRES.find((c) => c.id === id);
}

export function cadresParCategorie(categorie: string): CadreRegistre[] {
  return REGISTRE_CADRES.filter((c) => c.categorie === categorie);
}
