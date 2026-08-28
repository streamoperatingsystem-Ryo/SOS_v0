// Store communauté lecture (Helix). Followers, subs, viewers, broadcaster.
// 403 → communauteErreur = "need_reauth" → le frontend affiche le banner
// « Reconnecter Twitch pour la communauté » (bouton → reconnecterTwitch).
import { writable } from "svelte/store";
import { tauri } from "../tauri";

export interface FollowerEntry {
  login: string;
  user_id: string;
  followed_at: string;
}

export interface FollowersResp {
  total: number;
  liste: FollowerEntry[];
}

export interface SubEntry {
  login: string;
  user_id: string;
  tier: string;
  is_gift: boolean;
}

export interface SubsResp {
  total: number;
  points: number;
  liste: SubEntry[];
}

export interface BroadcasterInfo {
  display_name: string;
  profile_image_url: string;
  broadcaster_type: string;
  description: string;
}

export const followers = writable<FollowersResp | null>(null);
export const subs = writable<SubsResp | null>(null);
export const viewers = writable<number | null>(null);
export const broadcaster = writable<BroadcasterInfo | null>(null);

/// Erreur communauté. "need_reauth" = 403 (scopes manquants). null = OK.
export const communauteErreur = writable<string | null>(null);

/// Charge les followers. Sur "need_reauth" → communauteErreur = "need_reauth".
export async function chargerFollowers(): Promise<void> {
  console.log("[Communauté] chargerFollowers()...");
  try {
    const resp = await tauri.twitchCommunauteFollowers();
    followers.set(resp);
    communauteErreur.set(null);
    console.log("[Communauté] followers OK:", resp.total, resp.liste.length);
  } catch (e) {
    const msg = String(e);
    console.error("[Communauté] followers ERR:", msg);
    if (msg === "need_reauth") {
      communauteErreur.set("need_reauth");
    } else {
      communauteErreur.set(msg);
    }
  }
}

/// Charge les subs. Même gestion d'erreur.
export async function chargerSubs(): Promise<void> {
  console.log("[Communauté] chargerSubs()...");
  try {
    const resp = await tauri.twitchCommunauteSubs();
    subs.set(resp);
    communauteErreur.set(null);
    console.log("[Communauté] subs OK:", resp.total, resp.liste.length);
  } catch (e) {
    const msg = String(e);
    console.error("[Communauté] subs ERR:", msg);
    if (msg === "need_reauth") {
      communauteErreur.set("need_reauth");
    } else {
      communauteErreur.set(msg);
    }
  }
}

/// Charge les viewers live. None si hors-ligne.
export async function chargerViewers(): Promise<void> {
  console.log("[Communauté] chargerViewers()...");
  try {
    const resp = await tauri.twitchCommunauteViewers();
    viewers.set(resp);
    console.log("[Communauté] viewers OK:", resp);
  } catch (e) {
    const msg = String(e);
    console.error("[Communauté] viewers ERR:", msg);
    if (msg !== "need_reauth") {
      communauteErreur.set(msg);
    }
  }
}

/// Charge le broadcaster (display_name, avatar, type).
export async function chargerBroadcaster(): Promise<void> {
  console.log("[Communauté] chargerBroadcaster()...");
  try {
    const resp = await tauri.twitchBroadcaster();
    broadcaster.set(resp);
    console.log("[Communauté] broadcaster OK:", resp.display_name);
  } catch (e) {
    const msg = String(e);
    console.error("[Communauté] broadcaster ERR:", msg);
    if (msg === "need_reauth") {
      communauteErreur.set("need_reauth");
    }
  }
}

/// Charge tout en parallèle (Promise.allSettled). Agrège les erreurs.
export async function chargerCommunaute(): Promise<void> {
  console.log("[Communauté] chargerCommunaute() appelé");
  await Promise.allSettled([
    chargerBroadcaster(),
    chargerFollowers(),
    chargerSubs(),
    chargerViewers(),
  ]);
  console.log("[Communauté] chargerCommunaute() terminé");
}

/// Remet à zéro (après déconnexion).
export function resetCommunaute(): void {
  followers.set(null);
  subs.set(null);
  viewers.set(null);
  broadcaster.set(null);
  communauteErreur.set(null);
}
