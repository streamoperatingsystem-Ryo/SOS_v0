// Store présentation streamer : état Rust + actions + preview audio local.
// Écoute "presentation:etat" (émis par Rust après chaque mutation : CRUD
// favoris, show/hide, bascule de mode). Les contrôles live (clip, audio,
// fermer) passent par les commandes — pas besoin de refresh systématique,
// l'event tient le store à jour.
//
// Portage de la feature « streamers favoris » de l'ancienne app :
// déclenchement 100 % manuel depuis l'onglet « Présentation » de la modale
// Interactions chat — pas de détection automatique de message.
import { writable } from "svelte/store";
import { listen } from "@tauri-apps/api/event";
import {
  tauri,
  type PresentationEtat,
  type PresentationFavoriConfig,
  type PresentationDonnees,
  type PresentationSearchResult,
} from "../tauri";

/// Préfixe URL pour les médias servis par le serveur :4321 (dashboard ≠ :4321 —
/// même convention que Canvas.svelte / Widget.svelte / fonts.ts).
const MEDIA_BASE = "http://127.0.0.1:4321/";

export const presentationEtat = writable<PresentationEtat | null>(null);

let listenersBound = false;

/// Initialise le listener Tauri pour "presentation:etat".
/// Idempotent. À appeler au mount du dashboard.
export async function initPresentation(): Promise<void> {
  if (listenersBound) return;
  listenersBound = true;
  await listen<PresentationEtat>("presentation:etat", (e) => {
    presentationEtat.set(e.payload);
  });
}

/// Charge l'état initial (favoris + présentation en cours).
/// À appeler après initPresentation.
export async function chargerPresentation(): Promise<void> {
  try {
    const etat = await tauri.presentationEtat();
    presentationEtat.set(etat);
  } catch {
  }
}

// ===== Favoris =====

/// Ajoute/remplace un favori (upsert par login). L'event met le store à jour.
export async function presentationFavoriSauver(
  favori: PresentationFavoriConfig
): Promise<void> {
  await tauri.presentationFavoriSauver(favori);
}

export async function presentationFavoriSupprimer(login: string): Promise<void> {
  await tauri.presentationFavoriSupprimer(login);
}

/// Affecte/retire une piste audio ("voix"|"musique", src medias/… ou null).
export async function presentationFavoriSetSon(
  login: string,
  piste: string,
  src: string | null
): Promise<void> {
  await tauri.presentationFavoriSetSon(login, piste, src);
}

/// Change le mode ("theatrical"|"card") — bascule à chaud si en cours.
export async function presentationFavoriSetMode(
  login: string,
  mode: string
): Promise<void> {
  await tauri.presentationFavoriSetMode(login, mode);
}

// ===== Recherche =====

/// Recherche de chaînes Twitch (GET /search/channels — max 5 résultats).
/// Les erreurs remontent (la modale les affiche : need_reauth, déconnecté…).
export async function presentationRechercher(
  query: string
): Promise<PresentationSearchResult[]> {
  return tauri.presentationRechercher(query);
}

// ===== Cycle de vie =====

/// Lance la présentation d'un streamer (fetch Helix complet côté Rust →
/// frame presentation-show). Retourne les données (contrôles live).
export async function presentationLancer(
  userId: string,
  mode?: string
): Promise<PresentationDonnees> {
  return tauri.presentationLancer(userId, mode);
}

/// Ferme la présentation côté diffusion.
export async function presentationFermer(): Promise<void> {
  await tauri.presentationFermer();
}

/// Joue un clip sur la diffusion (résolution MP4 signée côté Rust — peut
/// prendre ~1 s la première fois, le cache Rust tient 10 min).
export async function presentationClipJouer(slug: string): Promise<void> {
  await tauri.presentationClipJouer(slug);
}

/// Arrête le clip en cours côté diffusion (retour à la carte).
export async function presentationClipStopper(): Promise<void> {
  await tauri.presentationClipStopper();
}

/// Joue une piste audio sur la diffusion. src = "medias/<uuid>.<ext>".
export async function presentationAudioJouer(
  piste: string,
  src: string
): Promise<void> {
  await tauri.presentationAudioJouer(piste, src);
}

/// Stoppe une piste audio ("voix"|"musique"|"toutes") sur la diffusion.
export async function presentationAudioStopper(piste: string): Promise<void> {
  await tauri.presentationAudioStopper(piste);
}

// ===== Preview audio locale (dashboard — le user entend sur son PC) =====

let audioLocal: HTMLAudioElement | null = null;

/// Écoute locale d'un son importé (bouton « Écouter » de la modale).
/// src = "medias/<uuid>.<ext>" → préfixé par le serveur :4321.
export function ecouterSon(src: string): void {
  stopperEcoute();
  const url = src.startsWith("http") ? src : `${MEDIA_BASE}${src}`;
  const audio = new Audio(url);
  audio.play().catch(() => {});
  audioLocal = audio;
}

/// Stoppe l'écoute locale courante (idempotent).
export function stopperEcoute(): void {
  if (audioLocal) {
    audioLocal.pause();
    audioLocal.currentTime = 0;
    audioLocal = null;
  }
}

/// Remet à zéro (après déconnexion / fermeture modale).
export function resetPresentation(): void {
  presentationEtat.set(null);
  stopperEcoute();
}
