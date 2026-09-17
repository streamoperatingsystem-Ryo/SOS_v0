// Registre central des cadres SVG — 7 cadres disponibles.
// Personnalisables (arrondis, carre, mgstyle) : rendus avec dégradé couleur
// utilisateur + angle de gradient personnalisable.
// Presets (cyberpunk, story, thin-line, hexagon) : renderer dédié.

import type { CadreOpts } from "./cadre-cyberpunk";
import renderCyberpunk, { getCyberpunkClipPath } from "./cadre-cyberpunk";
import renderStory, { getStoryClipPath } from "./cadre-story";
import renderThinLine, { getThinLineClipPath } from "./cadre-thin-line";
import renderHexagon, { getHexagonClipPath } from "./cadre-hexagon";
import renderMGStyle, { getMGStyleClipPath } from "./cadre-mgstyle";

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
  { id: "mgstyle", nom: "MGStyle", categorie: "tactique", variantes: null, personnalisable: true },

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
  mgstyle: renderMGStyle,
};

const MAP_CLIP_PATHS: Record<string, ClipPathFn> = {
  cyberpunk: getCyberpunkClipPath,
  story: getStoryClipPath,
  "thin-line": getThinLineClipPath,
  hexagon: getHexagonClipPath,
  mgstyle: getMGStyleClipPath,
};

/// Génère le SVG d'un cadre selon son style. Les cadres personnalisables
/// (arrondis, carre, mgstyle) sont rendus avec dégradé couleur/couleurFin +
/// angle de gradient utilisateur. Les presets sont dispatchés vers leur
/// renderer dédié.
export function genererSVG(
  style: string,
  variante: string | undefined,
  largeur: number,
  hauteur: number,
  couleur: string,
  couleurFin: string,
  strokeWidth: number,
  idCadre: string | undefined,
  gradientAngle: number | undefined
): string {
  const c = /^#[0-9a-f]{6}$/i.test(couleur || "") ? couleur : "#ffffff";
  const cFin = /^#[0-9a-f]{6}$/i.test(couleurFin || "") ? couleurFin : c;
  const ep = Number.isFinite(Number(strokeWidth)) ? Math.max(1, Number(strokeWidth)) : 4;
  const offset = ep / 2;
  const angle = Number.isFinite(Number(gradientAngle)) ? Number(gradientAngle) : 135;
  const { x1, y1, x2, y2 } = angleToGradientCoords(angle);

  // ── Cadres personnalisables inline (arrondis, carre) ──────────────────────
  if (style === "arrondis" || style === "carre") {
    const rx = style === "arrondis" ? 8 : 0;
    const gradId = `grad-cadre-${(idCadre || "d").replace(/[^a-zA-Z0-9_-]/g, "-")}-${style}`;
    return `<svg width="${largeur + offset * 2}" height="${hauteur + offset * 2}" viewBox="-${offset} -${offset} ${largeur + offset * 2} ${hauteur + offset * 2}" preserveAspectRatio="none" style="position:absolute;top:-${offset}px;left:-${offset}px;pointer-events:none;z-index:5;">
        <defs>
          <linearGradient id="${gradId}" x1="${x1}%" y1="${y1}%" x2="${x2}%" y2="${y2}%">
            <stop offset="0%" stop-color="${c}" stop-opacity="1"/>
            <stop offset="100%" stop-color="${cFin}" stop-opacity="1"/>
          </linearGradient>
        </defs>
        <rect x="0" y="0" width="${largeur}" height="${hauteur}" rx="${rx}" ry="${rx}" fill="none" stroke="url(#${gradId})" stroke-width="${ep}"/>
      </svg>`;
  }

  // ── Presets + mgstyle : dispatch ──────────────────────────────────────────
  const renderer = CADRES_MAP[style];
  if (renderer) {
    return renderer({ largeur, hauteur, idCadre, strokeWidth: ep, variante, couleur: c, couleurFin: cFin, gradientAngle: angle });
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

/// Convertit un angle en degrés (0-360) en coordonnées x1/y1/x2/y2 (0-100%)
/// pour un linearGradient SVG. 0° = gauche→droite, 90° = haut→bas,
/// 135° = diagonal haut-gauche→bas-droite (défaut).
function angleToGradientCoords(angle: number): { x1: number; y1: number; x2: number; y2: number } {
  const rad = (angle * Math.PI) / 180;
  const dx = Math.cos(rad);
  const dy = Math.sin(rad);
  // Normaliser vers 0-100% : projette le vecteur de direction sur le carré unitaire.
  const scale = 50 / Math.max(Math.abs(dx), Math.abs(dy), 0.001);
  const cx = 50, cy = 50;
  return {
    x1: Math.round(cx - dx * scale),
    y1: Math.round(cy - dy * scale),
    x2: Math.round(cx + dx * scale),
    y2: Math.round(cy + dy * scale),
  };
}

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

// Couleur représentative d'une variante de cadre preset (pastilles modale,
// badge « en lecture dans OBS »). Miroir des palettes internes des renderers.
const COULEURS_VARIANTES: Record<string, Record<string, string>> = {
  story: { pink: "#ff4d8d", blue: "#4d88ff", purple: "#8c4dff", green: "#4dff88", orange: "#ff884d", yellow: "#ffd700" },
  cyberpunk: { defaut: "#00f0ff", "vert-magenta": "#39ff14", "orange-cyan": "#ff8800", "violet-cyan": "#8c4dff", "rouge-cyan": "#ff2244" },
  "thin-line": { graphite: "#2d2d2d", argent: "#a0a0a0", "blanc-pure": "#ffffff" },
  hexagon: { ambre: "#f59e0b", azure: "#0ea5e9", emerald: "#10b981", violet: "#8b5cf6", steel: "#64748b" },
};

export function getVarianteColor(cadreId: string, varianteId: string): string {
  const couleurs = COULEURS_VARIANTES[cadreId];
  if (couleurs && couleurs[varianteId]) return couleurs[varianteId];
  return "#67e8f9";
}

/// Couleur principale d'un cadre : couleur utilisateur pour les cadres
/// personnalisables (arrondis, carre), couleur de la variante pour les
/// presets (leur rendu n'utilise PAS le champ couleur).
export function couleurCadre(style: string, variante: string | undefined, couleur: string | undefined): string {
  const cadre = trouverCadre(style);
  if (cadre?.personnalisable) return couleur || "#ffffff";
  return getVarianteColor(style, variante ?? "");
}

/// Paires de gradient (from → to) pour chaque variante de cadre preset.
/// Miroir des palettes internes des renderers (cadre-cyberpunk/story/thin-line/hexagon).
const GRADIENTS_VARIANTES: Record<string, Record<string, [string, string]>> = {
  cyberpunk: {
    defaut: ["#00f0ff", "#ff00ff"],
    "vert-magenta": ["#39ff14", "#ff00ff"],
    "orange-cyan": ["#ff8800", "#00f0ff"],
    "violet-cyan": ["#8c4dff", "#00f0ff"],
    "rouge-cyan": ["#ff2244", "#00f0ff"],
  },
  story: {
    pink: ["#ff4d8d", "#e63973"],
    blue: ["#4d88ff", "#3973e6"],
    purple: ["#8c4dff", "#7339e6"],
    green: ["#4dff88", "#39e673"],
    orange: ["#ff884d", "#e67339"],
    yellow: ["#ffd700", "#e6a800"],
  },
  "thin-line": {
    graphite: ["#2d2d2d", "#4a4a4a"],
    argent: ["#a0a0a0", "#c0c0c0"],
    "blanc-pure": ["#ffffff", "#e0e0e0"],
  },
  hexagon: {
    ambre: ["#f59e0b", "#b45309"],
    azure: ["#0ea5e9", "#0369a1"],
    emerald: ["#10b981", "#047857"],
    violet: ["#8b5cf6", "#6d28d9"],
    steel: ["#64748b", "#334155"],
  },
};

/// Dégradé de couleur d'un cadre pour le rendu du titre de widget.
/// Retourne { from, to } :
/// - Cadre personnalisable (arrondis, carre, mgstyle) → couleur/couleurFin du user.
/// - Preset → paire de couleurs de la variante (from → to).
/// - Pas de cadre actif (cadre null/inactif) → dégradé blanc→gris par défaut.
export function gradientCadre(
  cadre: { style: string; variante?: string; couleur: string; couleurFin: string; gradientAngle?: number; actif: boolean } | null | undefined
): { from: string; to: string } {
  if (!cadre || !cadre.actif) {
    return { from: "#e0e0e0", to: "#9a9a9a" };
  }
  const cadreReg = trouverCadre(cadre.style);
  if (cadreReg?.personnalisable) {
    return { from: cadre.couleur || "#ffffff", to: cadre.couleurFin || cadre.couleur || "#000000" };
  }
  const grads = GRADIENTS_VARIANTES[cadre.style];
  if (grads && cadre.variante && grads[cadre.variante]) {
    const [from, to] = grads[cadre.variante];
    return { from, to };
  }
  // Fallback : couleur unique de la variante → même couleur (dégradé plat).
  const c = getVarianteColor(cadre.style, cadre.variante ?? "");
  return { from: c, to: c };
}
