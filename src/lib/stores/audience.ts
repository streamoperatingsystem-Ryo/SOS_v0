// Store onglet « Audience » (modale Diagnostic) — sessions live reconstruites
// depuis telemetrie/events.jsonl par le backend (analyse.rs). PAS de polling :
// données historiques — re-fetch au changement de période ou de session.
import { get, writable } from "svelte/store";
import { tauri, type AudienceSessions, type AudienceSessionDetail } from "../tauri";

/// Réponse audience_sessions (sessions + agrégats) — null = jamais chargé.
export const audienceData = writable<AudienceSessions | null>(null);

/// Détail de la session sélectionnée (série + segments + pics + timeline).
export const audienceDetail = writable<AudienceSessionDetail | null>(null);

/// true quand le dernier invoke a échoué.
export const audienceErreur = writable(false);

/// Chargements en cours (liste / détail — séparés pour les états d'affichage).
export const audienceChargement = writable(false);
export const audienceChargementDetail = writable(false);

/// Période affichée en jours (défaut 30 — sélecteur 7/30/90).
export const audiencePeriode = writable(30);

/// Session sélectionnée (id d'intervalle — persiste entre onglets/montages).
export const audienceSessionId = writable<string | null>(null);

/// Pic sélectionné dans la liste (surligné sur le graphe) — index dans
/// detail.pics, null = aucun.
export const audiencePicSel = writable<number | null>(null);

/// Charge la liste des sessions + agrégats sur `jours` jours. Préserve la
/// session sélectionnée si elle existe encore dans la nouvelle liste, sinon
/// sélectionne la plus récente (la liste est triée récent → ancien).
export async function chargerAudience(jours: number): Promise<void> {
  audiencePeriode.set(jours);
  audienceChargement.set(true);
  try {
    const d = await tauri.audienceSessions(jours);
    audienceData.set(d);
    audienceErreur.set(false);
    const cur = get(audienceSessionId);
    if (cur && d.sessions.some((s) => s.session_id === cur)) return;
    const premiere = d.sessions[0]?.session_id ?? null;
    audienceSessionId.set(premiere);
    if (premiere) {
      await chargerSession(premiere);
    } else {
      audienceDetail.set(null);
    }
  } catch {
    audienceErreur.set(true);
  } finally {
    audienceChargement.set(false);
  }
}

/// Charge le détail d'une session (série + pics + timeline).
export async function chargerSession(id: string): Promise<void> {
  audienceSessionId.set(id);
  audiencePicSel.set(null);
  audienceChargementDetail.set(true);
  try {
    audienceDetail.set(await tauri.audienceSession(id));
    audienceErreur.set(false);
  } catch {
    audienceErreur.set(true);
  } finally {
    audienceChargementDetail.set(false);
  }
}
