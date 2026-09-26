// Store écran d'attente (BRB) : état config + runtime + listeners events Rust.
//
// Porté de l'ancienne app (features/ecran-attente/ecran-attente-store.js),
// réécrit pour l'architecture v0 : le chrono est détenu par Rust (tâche tokio
// 1 Hz — source de vérité) ; ce store ne fait que refléter l'événement
// `ecran-attente:etat` (config + actif + en_pause + temps_restant, émis à
// chaque tick/transition) et exposer les actions des commandes.
//
// Le média (image/vidéo) n'est JAMAIS lu côté dashboard — aucun élément
// audio/vidéo local : l'affichage et le son sont portés par la diffusion
// :4321 (consigne utilisateur).
import { writable, get } from "svelte/store";
import { listen } from "@tauri-apps/api/event";
import { tauri, type EcranAttenteConfig, type EcranAttenteEtat } from "../tauri";
import { obsHost, obsPort, obsPassword } from "./obs";

/// Snapshot complet (config + runtime). null = pas encore chargé.
export const ecranAttenteStore = writable<EcranAttenteEtat | null>(null);

/// Erreur action (import refusé, OBS injoignable pour le picker…).
/// null = pas d'erreur.
export const ecranAttenteErreur = writable<string | null>(null);

let listenersBound = false;

/// Initialise le listener Tauri. Idempotent. À appeler au mount du dashboard.
export async function initEcranAttente(): Promise<void> {
  if (listenersBound) return;
  listenersBound = true;

  // État complet à chaque tick 1 Hz / transition (lancer, pause, arrêt,
  // set_config, fin auto du chrono).
  await listen<EcranAttenteEtat>("ecran-attente:etat", (e) => {
    ecranAttenteStore.set(e.payload);
  });
}

/// Charge l'état initial. À appeler après initEcranAttente.
export async function chargerEcranAttente(): Promise<void> {
  try {
    ecranAttenteStore.set(await tauri.ecranAttenteEtat());
  } catch {
  }
}

// =============================================================================
// ACTIONS (wrappers tauri → l'état revient via l'event + refresh)
// =============================================================================

/// Creds OBS des stores (localStorage — partagés avec la modale OBS).
/// Le routage audio côté Rust est non-fatal si OBS est absent.
function obsCreds(): { host: string; port: number; password: string } {
  return {
    host: get(obsHost),
    port: parseInt(get(obsPort), 10) || 4455,
    password: get(obsPassword),
  };
}

async function refresh(): Promise<void> {
  try {
    ecranAttenteStore.set(await tauri.ecranAttenteEtat());
  } catch {
  }
}

/// Remplace la config + persiste (re-émet la frame WS si l'écran est actif).
export async function ecranAttenteSetConfig(
  config: EcranAttenteConfig,
): Promise<void> {
  await tauri.ecranAttenteSetConfig(config);
  await refresh();
}

/// Lance l'écran d'attente (chrono + overlay diffusion + routage audio OBS).
export async function lancerEcranAttente(): Promise<void> {
  const { host, port, password } = obsCreds();
  await tauri.ecranAttenteLancer(host, port, password);
  await refresh();
}

/// Bascule pause/reprise du chrono.
export async function togglePauseEcranAttente(): Promise<void> {
  await tauri.ecranAttentePauseToggle();
  await refresh();
}

/// Arrête l'écran + restaure l'audio bureau OBS.
export async function arreterEcranAttente(): Promise<void> {
  const { host, port, password } = obsCreds();
  await tauri.ecranAttenteArreter(host, port, password);
  await refresh();
}

/// Toggle start/stop — pour la barre de raccourcis (bouton unique).
export async function toggleEcranAttente(): Promise<void> {
  const actif = get(ecranAttenteStore)?.actif ?? false;
  if (actif) {
    await arreterEcranAttente();
  } else {
    await lancerEcranAttente();
  }
}

/// Importe un média (image OU vidéo) : dialog → validation/copie Rust vers
/// medias/ → pose media/media_kind/media_nom dans la config + persiste.
/// Retourne true si un média a été importé, false si dialog annulé.
/// Lance l'erreur (taille/format) pour affichage dans la modale.
export async function importerMediaEcranAttente(): Promise<boolean> {
  const res = await tauri.ecranAttenteImporterMedia();
  if (!res) return false;
  const [rel, kind, nom] = res;
  const etat = await tauri.ecranAttenteEtat();
  await ecranAttenteSetConfig({
    ...etat.config,
    media: rel,
    media_kind: kind,
    media_nom: nom,
  });
  return true;
}

/// Retire le média de la config (persiste).
export async function retirerMediaEcranAttente(): Promise<void> {
  const etat = await tauri.ecranAttenteEtat();
  await ecranAttenteSetConfig({
    ...etat.config,
    media: undefined,
    media_kind: undefined,
    media_nom: undefined,
  });
}

/// Liste les sources audio de sortie OBS pour le picker de la modale.
/// Connexion OBS one-shot — lève une erreur si OBS absent (à afficher).
export async function listerSourcesAudioEcranAttente(): Promise<string[]> {
  const { host, port, password } = obsCreds();
  return tauri.ecranAttenteListerSourcesAudio(host, port, password);
}
