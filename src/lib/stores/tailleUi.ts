// Taille des polices de l'interface dashboard : font-size sur <html> =
//   16 px × tailleUi (select topbar, ajustement relatif persisté)
//         × tailleEcran (base auto proportionnelle à la GRANDE DIMENSION
//           LOGIQUE du moniteur courant — physique ÷ scaleFactor : les
//           CSS px de WebView2 sont déjà multipliés par le scaling
//           Windows, compter les px physiques double-compterait le zoom
//           OS —, invariante à la rotation, en 2 segments :
//           ≤ 1920 : ancrée 1366 → ~100 %, 720p paysage → 95 %,
//             saturée à 125 % dès ~1690 (1080p → 125 %) ;
//           > 1920 : proportion stricte ancrée 1920 → 125 % — la
//             proportion chrome/écran est IDENTIQUE sur 1080p, 2K et 4K
//             (2560 → ~165 %, 3840 → 250 % plafond 4K).
// Tous les textes du chrome étant en rem, toute l'UI suit (le canvas
// WYSIWYG en px/cqw n'est pas affecté). tailleUi persisté en localStorage
// PAR RÉSOLUTION d'écran (clé "WxH" — deux moniteurs identiques partagent
// le réglage) ; tailleEcran est runtime-only (dépend du hardware) et se
// recalcule quand la fenêtre change d'écran (onMoved/onScaleChanged —
// drag souris ou saut via le sélecteur Ergonomie).
import { writable, type Writable } from "svelte/store";
import { currentMonitor, getCurrentWindow } from "@tauri-apps/api/window";

/// Niveaux proposés par le select de la topbar (pourcentages, 80 % → 125 %).
/// Les boutons de la topbar sont en rem : ils suivent l'échelle, le
/// libellé reste proportionnel à tous les niveaux.
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
      // Avant initTailleUi : ne JAMAIS écraser une map par un nombre nu
      // (le set initial du module écrirait "100" par-dessus
      // {"1920x1080":110} et perdrait le cran de chaque écran) —
      // fusionner sous la clé "" (repli legacy) si le LS est déjà une
      // map ; format nombre-seul conservé sinon.
      const p: unknown = JSON.parse(localStorage.getItem(LS_TAILLE) ?? "null");
      if (p && typeof p === "object") {
        (p as Record<string, number>)[""] = v;
        localStorage.setItem(LS_TAILLE, JSON.stringify(p));
      } else {
        localStorage.setItem(LS_TAILLE, String(v));
      }
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

/// Ancrages de la proportion (grande dimension logique) :
///   segment ≤ 1920 : 1080 px → 80 % (≡ 1366 → ~100 %, validé par
///     l'utilisateur sur son écran portrait 768×1366) ;
///   segment > 1920 : 1920 px → 125 % — continuité avec le plafond du
///     segment bas, puis proportion stricte (2560 → 165 %, 3840 → 250 %)
///     pour que le chrome garde la même proportion d'écran en 2K/4K.
const ANCRE_DIM = 1080;
const ANCRE_PCT = 80;
const ANCRE_HAUT_DIM = 1920;
const ANCRE_HAUT_PCT = 125;
/// Bornes de la base écran (%) : plancher lisibilité sur petits écrans ;
/// le segment ≤ 1920 sature à 125 % dès ~1690 px (1080p) ; au-delà,
/// plafond global 250 % (4K @ scaling 100 %).
const TAILLE_ECRAN_MIN = 55;
const TAILLE_ECRAN_MAX = 250;
const TAILLE_ECRAN_DEFAUT = 100;

/// Base « écran » de la taille UI, en % : proportion de la GRANDE
/// DIMENSION logique du moniteur où se trouve la fenêtre main
/// (max(largeur, hauteur) — invariante à la rotation : un écran portrait
/// garde la même base qu'en paysage, sa hauteur étant sa grande
/// dimension). Runtime-only — recalculée par initTailleUi au boot puis à
/// chaque changement de moniteur.
export const tailleEcran: Writable<number> = writable(TAILLE_ECRAN_DEFAUT);

/// Base écran (%) pour une résolution LOGIQUE de moniteur : proportion
/// de sa grande dimension, en 2 segments (1080 → 80 % jusqu'à 1920,
/// saturée à 125 % ; puis 1920 → 125 % stricte), arrondie au pas de 5,
/// bornée 55–250.
function tailleEcranDepuisDims(largeur: number, hauteur: number): number {
  const dim = Math.max(largeur, hauteur);
  const brut =
    dim <= ANCRE_HAUT_DIM
      ? Math.min(ANCRE_HAUT_PCT, (ANCRE_PCT * dim) / ANCRE_DIM)
      : (ANCRE_HAUT_PCT * dim) / ANCRE_HAUT_DIM;
  const p = Math.round(brut / 5) * 5;
  return Math.min(TAILLE_ECRAN_MAX, Math.max(TAILLE_ECRAN_MIN, p));
}

/// Calcule la base écran depuis le moniteur courant puis la recalcule à
/// chaque changement de moniteur (drag fenêtre, saut via le sélecteur
/// Ergonomie — les deux émettent des events de déplacement — et
/// changement de scale factor). Appelé une fois dans App.svelte onMount,
/// AVANT "frontend:pret" : la fenêtre apparaît déjà à la bonne taille.
/// Fallbacks : currentMonitor() ko → window.screen (déjà logique) →
/// 1080p paysage (1920×1080 → 125 %).
export async function initTailleUi(): Promise<void> {
  const win = getCurrentWindow();
  async function maj() {
    try {
      const m = await currentMonitor();
      // devicePixelRatio : meilleur proxy du scaleFactor quand
      // currentMonitor() renvoie null (window.screen est déjà logique).
      const sf = m?.scaleFactor || window.devicePixelRatio || 1;
      const wPhys = m?.size.width || window.screen.width * sf || 1920;
      const hPhys = m?.size.height || window.screen.height * sf || 1080;
      // tailleEcran = dimension LOGIQUE (physique ÷ scaleFactor).
      tailleEcran.set(tailleEcranDepuisDims(wPhys / sf, hPhys / sf));
      // Réglage utilisateur de CET écran — persisté par résolution
      // PHYSIQUE : l'identité du moniteur ne change pas quand le
      // scaling Windows bouge (repli legacy/défaut si jamais réglé).
      // Poser la clé AVANT le set pour que le subscribe réécrive sous
      // la bonne entrée.
      cleEcranCourante = `${wPhys}x${hPhys}`;
      tailleUi.set(lireLS(cleEcranCourante));
    } catch {
      tailleEcran.set(
        tailleEcranDepuisDims(
          window.screen.width || 1920,
          window.screen.height || 1080
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
