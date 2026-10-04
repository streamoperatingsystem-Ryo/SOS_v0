// Store langue UI (i18n dashboard) : dictionnaires JSON par langue (src/lib/i18n/),
// traduction via $t("cle") dans les templates Svelte et get(t)("cle") dans le TS.
// - fr.json = source de vérité : toute clé existe d'abord en français.
// - Fallback : langue choisie → fr → la clé elle-même (visible en dev).
// - Choix persisté dans langue.json (%APPDATA%) via les commandes Rust
//   langue_etat / langue_sauver (pattern onboarding.json).
// - Périmètre v1 : DASHBOARD UNIQUEMENT — diffusion.html / raccourcis.html /
//   chat-popout.html ne sont PAS traduits (pages OBS autonomes).
import { writable, derived } from "svelte/store";
import { tauri } from "../tauri";
import fr from "../i18n/fr.json";
import en from "../i18n/en.json";
import de from "../i18n/de.json";
import es from "../i18n/es.json";
import it from "../i18n/it.json";
import ru from "../i18n/ru.json";
import zh from "../i18n/zh.json";
import ja from "../i18n/ja.json";

/// Langues supportées par le dashboard. Ajouter une langue = un nouveau
/// fichier src/lib/i18n/<code>.json + une entrée ici + LOCALE_TAGS ci-dessous
/// (aucun changement Rust — langue_sauver n'a pas de whitelist).
export type Langue = "fr" | "en" | "de" | "es" | "it" | "ru" | "zh" | "ja";

/// Langues proposées dans le sélecteur topbar (noms NATIFS — jamais traduits :
/// un utilisateur doit toujours retrouver sa langue écrite chez lui).
export const LANGUES: { id: Langue; nom: string }[] = [
  { id: "en", nom: "English" },
  { id: "fr", nom: "Français" },
  { id: "zh", nom: "中文" },
  { id: "de", nom: "Deutsch" },
  { id: "es", nom: "Español" },
  { id: "it", nom: "Italiano" },
  { id: "ru", nom: "Русский" },
  { id: "ja", nom: "日本語" },
];

/// Tag locale complet pour toLocaleString / Intl (formatage des nombres
/// des widgets objectif : 1 234 vs 1,234). Suit la langue sélectionnée.
const LOCALE_TAGS: Record<Langue, string> = {
  fr: "fr-FR",
  en: "en-US",
  de: "de-DE",
  es: "es-ES",
  it: "it-IT",
  ru: "ru-RU",
  zh: "zh-CN",
  ja: "ja-JP",
};

type Dict = Record<string, unknown>;

const DICTS: Record<Langue, Dict> = { fr, en, de, es, it, ru, zh, ja };

/// Langue courante. "fr" au boot, remplacée par initLangue() (langue.json →
/// navigator.language → "fr") dès le onMount d'App.svelte.
export const langue = writable<Langue>("fr");

/// Tag locale de la langue courante (ex: "fr-FR") — pour les toLocaleString.
export const localeTag = derived(langue, (l) => LOCALE_TAGS[l]);

/// Résout une clé pointée "zone.element.variante" dans un dictionnaire
/// imbriqué. undefined si la clé n'existe pas (→ fallback fr).
function chercher(dict: Dict, cle: string): string | undefined {
  let cur: unknown = dict;
  for (const part of cle.split(".")) {
    if (cur === null || typeof cur !== "object") return undefined;
    cur = (cur as Dict)[part];
  }
  return typeof cur === "string" ? cur : undefined;
}

/// Traduit une clé avec variables optionnelles : t("cle", { nom }) remplace
/// chaque {nom} dans la chaîne. Ordre : langue courante → fr → clé brute.
function traduire(l: Langue, cle: string, vars?: Record<string, string | number>): string {
  let s = chercher(DICTS[l], cle) ?? chercher(DICTS.fr, cle) ?? cle;
  if (vars) {
    for (const [k, v] of Object.entries(vars)) {
      s = s.replaceAll(`{${k}}`, String(v));
    }
  }
  return s;
}

/// Traducteur réactif. Template Svelte : {$t("toolbar.widgets.creer")} —
/// Code TS : get(t)("cle") (import { get } from "svelte/store").
export const t = derived(langue, (l) => (cle: string, vars?: Record<string, string | number>) =>
  traduire(l, cle, vars)
);

/// Détecte la langue navigateur (prefix 2 lettres) si elle fait partie des
/// langues supportées, sinon "fr".
function langueNavigateur(): Langue {
  const nav = (navigator.language ?? "fr").slice(0, 2).toLowerCase();
  return (LANGUES.some((l) => l.id === nav) ? nav : "fr") as Langue;
}

/// Charge la langue au boot : langue.json (choix explicite persisté) →
/// navigator.language → "fr". Non-fatal : erreur invoke → langue navigateur.
export async function initLangue(): Promise<void> {
  let sauvee: string | null = null;
  try {
    sauvee = await tauri.langueEtat();
  } catch (e) {
    console.error("initLangue etat:", e);
  }
  if (sauvee && LANGUES.some((l) => l.id === sauvee)) {
    langue.set(sauvee as Langue);
  } else {
    langue.set(langueNavigateur());
  }
}

/// Change la langue : store immédiat (l'UI bascule instantanément) + persistance
/// langue.json non-fatale (erreur → console, l'UI reste dans la nouvelle langue).
export async function setLangue(l: Langue): Promise<void> {
  langue.set(l);
  try {
    await tauri.langueSauver(l);
  } catch (e) {
    console.error("setLangue sauver:", e);
  }
}
