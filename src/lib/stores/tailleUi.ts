// Taille des polices de l'interface dashboard : font-size sur <html> =
//   16 px × tailleUi (select topbar, ajustement relatif persisté)
//         × tailleEcran (base auto proportionnelle à la GRANDE DIMENSION
//           physique du moniteur courant — invariante à la rotation —,
//           ancrée 1366 → ~100 % : 720p paysage → 95 %, paysage ≥ ~1690
//           dont 1080p/4K → 125 % plafond).
// Tous les textes du chrome étant en rem, toute l'UI suit (le canvas
// WYSIWYG en px/cqw n'est pas affecté). tailleUi persisté en localStorage
// PAR RÉSOLUTION d'écran (clé "WxH" — deux moniteurs identiques partagent
// le réglage) ; tailleEcran est runtime-only (dépend du hardware) et se
// recalcule quand la fenêtre change d'écran (onMoved/onScaleChanged —
// drag souris ou saut via le sélecteur Ergonomie).
import { writable, type Writable } from "svelte/store";
import { currentMonitor, getCurrentWindow } from "@tauri-apps/api/window";

/// Niveaux proposés par le select de la topbar (pourcentages, 80 % → 125 %).
/// Borné pour que le texte reste dans les boutons fixes de 32 px de la
/// topbar (à 125 % : 0.8 rem ≈ 16 px de texte).
export const NIVEAUX_TAILLE_UI = [80, 85, 90, 95, 100, 105, 110, 115, 120, 125];
export const TAILLE_UI_DEFAUT = 100;

const LS_TAILLE = "sos-taille-ui";

/// Borné à NIVEAUX_TAILLE_UI — une valeur hors liste retombe sur undefined.
function borne(v: unknown): number | undefined {
  return typeof v === "number" && NIVEAUX_TAILLE_UI.includes(v)
    ? v
    : undefined;
}

/// Lit le niveau persisté pour une clé écran ("WxH"). Persistance =
/// objet JSON { "768x1366": 100, … } ; la clé "" abrite l'ancienne valeur
/// globale (legacy nombre seul, migrée à la première écriture) et sert de
/// repli pour les écrans jamais réglés. Sans clé (avant initTailleUi) :
/// repli legacy → défaut.
function lireLS(cle?: string): number {
  try {
    const p: unknown = JSON.parse(localStorage.getItem(LS_TAILLE) ?? "null");
    if (typeof p === "number") return borne(p) ?? TAILLE_UI_DEFAUT;
    if (p && typeof p === "object") {
      const map = p as Record<string, unknown>;
      return (
        (cle ? borne(map[cle]) : undefined) ??
        borne(map[""]) ??
        TAILLE_UI_DEFAUT
      );
    }
  } catch {
    // JSON invalide / localStorage indisponible (webview restreinte)
  }
  return TAILLE_UI_DEFAUT;
}

/// Niveau courant en % (100 = taille normale). Écrit par le select de la
/// topbar — les valeurs possibles sont déjà bornées par les <option> — et
/// ré-appliqué par initTailleUi à chaque changement de moniteur.
export const tailleUi: Writable<number> = writable(lireLS());

/// Clé "WxH" du moniteur courant — posée par initTailleUi. Vide avant :
/// les écritures éventuelles gardent le format legacy (nombre seul).
let cleEcranCourante = "";

tailleUi.subscribe((v) => {
  try {
    if (!cleEcranCourante) {
      localStorage.setItem(LS_TAILLE, String(v));
      return;
    }
    const p: unknown = JSON.parse(localStorage.getItem(LS_TAILLE) ?? "{}");
    // Migration douce : un ancien réglage global (nombre) devient la clé
    // "" = repli pour les écrans jamais réglés.
    const map: Record<string, number> =
      typeof p === "number"
        ? { "": p }
        : p && typeof p === "object"
          ? (p as Record<string, number>)
          : {};
    map[cleEcranCourante] = v;
    localStorage.setItem(LS_TAILLE, JSON.stringify(map));
  } catch {
    // ignore (quota / mode privé) — la taille marche quand même en RAM
  }
});

// ===== Base écran (proportion de la résolution) =====

/// Ancrage de la proportion : grande dimension 1080 px → base 80 %
/// (équivalent : 1366 px → ~100 % — résultat validé par l'utilisateur sur
/// son écran portrait 768×1366).
const ANCRE_DIM = 1080;
const ANCRE_PCT = 80;
/// Bornes de la base écran (%) : plancher lisibilité sur petits écrans ;
/// plafond 125 % — la proportion est stricte jusqu'à ~1690 px de grande
/// dimension puis sature (1080p paysage et au-delà → 125 %).
const TAILLE_ECRAN_MIN = 55;
const TAILLE_ECRAN_MAX = 125;
const TAILLE_ECRAN_DEFAUT = 100;

/// Base « écran » de la taille UI, en % : proportion de la GRANDE
/// DIMENSION physique du moniteur où se trouve la fenêtre main
/// (max(largeur, hauteur) — invariante à la rotation : un écran portrait
/// garde la même base qu'en paysage, sa hauteur étant sa grande
/// dimension). Runtime-only — recalculée par initTailleUi au boot puis à
/// chaque changement de moniteur.
export const tailleEcran: Writable<number> = writable(TAILLE_ECRAN_DEFAUT);

/// Base écran (%) pour une résolution physique de moniteur : proportion
/// de sa grande dimension à l'ancre 1080 px → 80 %, arrondie au pas de 5
/// et bornée 55–125.
function tailleEcranDepuisDims(largeur: number, hauteur: number): number {
  const p = Math.round(((ANCRE_PCT * Math.max(largeur, hauteur)) / ANCRE_DIM) / 5) * 5;
  return Math.min(TAILLE_ECRAN_MAX, Math.max(TAILLE_ECRAN_MIN, p));
}

/// Calcule la base écran depuis le moniteur courant puis la recalcule à
/// chaque changement de moniteur (drag fenêtre, saut via le sélecteur
/// Ergonomie — les deux émettent des events de déplacement — et
/// changement de scale factor). Appelé une fois dans App.svelte onMount,
/// AVANT "frontend:pret" : la fenêtre apparaît déjà à la bonne taille.
/// Fallbacks : currentMonitor() ko → window.screen × devicePixelRatio
/// (≈ physique) → 1080p paysage (1920×1080 → 125 %).
export async function initTailleUi(): Promise<void> {
  const win = getCurrentWindow();
  async function maj() {
    try {
      const m = await currentMonitor();
      const w = m?.size.width || window.screen.width * window.devicePixelRatio || 1920;
      const h = m?.size.height || window.screen.height * window.devicePixelRatio || 1080;
      tailleEcran.set(tailleEcranDepuisDims(w, h));
      // Réglage utilisateur de CET écran (persisté par résolution —
      // repli legacy/défaut si jamais réglé). Poser la clé AVANT le set
      // pour que le subscribe réécrive sous la bonne entrée.
      cleEcranCourante = `${w}x${h}`;
      tailleUi.set(lireLS(cleEcranCourante));
    } catch {
      tailleEcran.set(
        tailleEcranDepuisDims(
          window.screen.width * window.devicePixelRatio || 1920,
          window.screen.height * window.devicePixelRatio || 1080
        )
      );
    }
  }
  await maj();
  // Debounce : les events move fusent pendant le drag et chaque
  // currentMonitor() est un invoke — pas de spam.
  let timer: ReturnType<typeof setTimeout> | undefined;
  const debounced = () => {
    clearTimeout(timer);
    timer = setTimeout(() => void maj(), 150);
  };
  try {
    await win.onMoved(debounced);
    await win.onScaleChanged(debounced);
  } catch (e) {
    console.error("initTailleUi listeners:", e);
  }
}
