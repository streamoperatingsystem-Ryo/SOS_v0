// Store planning Twitch (schedule Helix — onglet « Planning » de la modale
// « Interactions Live »). Twitch = source de vérité, zéro persistance locale :
// Helix est relu à chaque chargement, et l'UI rappelle chargerPlanning()
// après chaque mutation (création/modif/suppression/vacances).
// Erreur "need_reauth" = 403 (vieux token sans channel:manage:schedule) →
// banner reconnexion, JAMAIS de device flow auto.
//
// initPlanning() enregistre le listener de déconnexion UNE FOIS au boot
// (App.svelte, à côté de initTwitchLive).
import { get, writable } from "svelte/store";
import { listen } from "@tauri-apps/api/event";
import {
  tauri,
  type PlanningResp,
  type PlanningSegment,
  type PlanningSegmentPatch,
} from "../tauri";
import { broadcaster, chargerBroadcaster } from "./communaute";

export type { PlanningResp, PlanningSegment, PlanningSegmentPatch };

/// Segments lus depuis Helix (occurrences — un récurrent sort une entrée
/// par semaine, chacune avec son propre id Helix).
export const planningSegments = writable<PlanningSegment[]>([]);
/// Vacances déclarées (dates ISO). null = pas de vacances (champs vides).
export const planningVacances = writable<{ start: string; end: string } | null>(
  null
);
/// Cache des jacquettes par category_id — rempli au chargement (GET /games
/// pour les segments) et à la recherche (résultats /search/categories).
/// box_art_url brute (placeholders {width}x{height} — remplacés au rendu).
export const planningJacquettes = writable<Record<string, string>>({});
/// GET /schedule en cours.
export const planningChargement = writable<boolean>(false);
/// Erreur de l'onglet. "need_reauth" = 403 → banner reconnexion.
export const planningErreur = writable<string | null>(null);

/// Fuseau IANA du poste (ex. Europe/Paris) — Twitch l'exige à la création
/// d'un segment et pour les vacances.
export function planningTimezone(): string {
  return Intl.DateTimeFormat().resolvedOptions().timeZone;
}

function erreur(e: unknown): void {
  const msg = String(e);
  planningErreur.set(msg === "need_reauth" ? "need_reauth" : msg);
}

/// Récupère les jacquettes des category_id pas encore en cache
/// (GET /games?id= — pas de batch dans le wrapper, quelques ids max).
async function remplirJacquettes(segments: PlanningSegment[]): Promise<void> {
  const cache = get(planningJacquettes);
  const ids = [
    ...new Set(
      segments.map((s) => s.category_id).filter((id) => id && !cache[id])
    ),
  ];
  if (ids.length === 0) return;
  const ajouts: Record<string, string> = {};
  await Promise.allSettled(
    ids.map(async (id) => {
      const jeu = await tauri.twitchLiveGameInfo(id);
      if (jeu?.box_art_url) ajouts[id] = jeu.box_art_url;
    })
  );
  if (Object.keys(ajouts).length > 0) {
    planningJacquettes.update((c) => ({ ...c, ...ajouts }));
  }
}

/// Relit Helix : segments à partir de `startTime` (RFC3339, borne basse de
/// la vue courante) + vacances + jacquettes manquantes + broadcaster_type
/// (gate des segments ponctuels — Twitch les refuse aux non-affiliés).
/// Appelé à l'ouverture de l'onglet, à chaque navigation de vue et après
/// chaque mutation par le composant.
export async function chargerPlanning(startTime?: string): Promise<void> {
  planningChargement.set(true);
  planningErreur.set(null);
  try {
    const resp = await tauri.twitchPlanningSchedule(startTime);
    planningSegments.set(resp.segments);
    planningVacances.set(
      resp.vacation_start
        ? { start: resp.vacation_start, end: resp.vacation_end }
        : null
    );
    const taches: Promise<unknown>[] = [remplirJacquettes(resp.segments)];
    if (!get(broadcaster)) taches.push(chargerBroadcaster());
    await Promise.allSettled(taches);
  } catch (e) {
    erreur(e);
  } finally {
    planningChargement.set(false);
  }
}

// ===== Actions (relais des commandes twitch_planning_*) =====

/// Crée un segment. Le backend tente le ponctuel quand demandé et retombe
/// sur récurrent si Twitch refuse (chaîne non affiliée). Retourne le
/// segment créé — l'UI rappelle ensuite chargerPlanning().
export async function planningCreerSegment(opts: {
  startTime: string;
  isRecurring: boolean;
  durationMin: number;
  categoryId?: string;
  title?: string;
}): Promise<PlanningSegment> {
  try {
    const seg = await tauri.twitchPlanningSegmentCreer({
      ...opts,
      timezone: planningTimezone(),
    });
    planningErreur.set(null);
    return seg;
  } catch (e) {
    erreur(e);
    throw e;
  }
}

/// Modifie un segment (PATCH partiel — clés snake_case format Helix).
/// ⚠️ Sur un récurrent, s'applique à toute la série (confirmé dans l'UI).
/// L'UI rappelle ensuite chargerPlanning().
export async function planningMajSegment(
  id: string,
  patch: PlanningSegmentPatch
): Promise<void> {
  try {
    await tauri.twitchPlanningSegmentMaj(id, patch);
    planningErreur.set(null);
  } catch (e) {
    erreur(e);
    throw e;
  }
}

/// Supprime un segment. ⚠️ Sur un récurrent, toute la série est supprimée
/// (confirmé dans l'UI). L'UI rappelle ensuite chargerPlanning().
export async function planningSupprimerSegment(id: string): Promise<void> {
  try {
    await tauri.twitchPlanningSegmentSupprimer(id);
    planningErreur.set(null);
  } catch (e) {
    erreur(e);
    throw e;
  }
}

/// Définit/désactive les vacances. `debut`/`fin` = YYYY-MM-DD (input date)
/// — convertis en RFC3339 UTC avant l'envoi (format attendu par Helix) :
/// minuit local pour le début, 23:59:59 local pour la fin.
/// L'UI rappelle ensuite chargerPlanning().
export async function planningSetVacances(
  actif: boolean,
  debut?: string,
  fin?: string
): Promise<void> {
  try {
    await tauri.twitchPlanningVacances({
      actif,
      debut: debut
        ? new Date(`${debut}T00:00:00`).toISOString()
        : undefined,
      fin: fin ? new Date(`${fin}T23:59:59`).toISOString() : undefined,
      timezone: planningTimezone(),
    });
    planningErreur.set(null);
  } catch (e) {
    erreur(e);
    throw e;
  }
}

/// Recherche de catégories Twitch (relais de la commande « En direct » —
/// les category_id retournés sont directement utilisables par l'API
/// schedule). La jacquette de chaque résultat est mise en cache pour
/// l'affichage du segment une fois créé.
export async function planningRechercherCategories(
  query: string
): Promise<{ id: string; name: string; box_art_url: string }[]> {
  try {
    const res = await tauri.twitchLiveSearchCategories(query);
    const ajouts: Record<string, string> = {};
    for (const c of res) if (c.box_art_url) ajouts[c.id] = c.box_art_url;
    planningJacquettes.update((c) => ({ ...c, ...ajouts }));
    return res;
  } catch (e) {
    erreur(e);
    throw e;
  }
}

/// Remet tout à zéro (déconnexion Twitch).
export function resetPlanning(): void {
  planningSegments.set([]);
  planningVacances.set(null);
  planningJacquettes.set({});
  planningChargement.set(false);
  planningErreur.set(null);
}

let initialized = false;

/// Enregistre le listener de déconnexion UNE FOIS — à appeler au boot
/// (App.svelte, à côté de initTwitchLive) : les segments/vacances affichés
/// deviennent obsolètes dès que le compte Twitch se déconnecte.
export async function initPlanning(): Promise<void> {
  if (initialized) return;
  initialized = true;
  await listen("twitch:deconnecte", () => resetPlanning());
}
