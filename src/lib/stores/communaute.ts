// Store communauté lecture (Helix). Followers, subs, viewers, broadcaster.
// 403 → communauteErreur = "need_reauth" → le frontend affiche le banner
// « Reconnecter Twitch pour la communauté » (bouton → reconnecterTwitch).
import { writable } from "svelte/store";
import { tauri } from "../tauri";

export interface FollowerEntry {
  login: string;
  user_id: string;
  followed_at: string;
  display_name: string;
  profile_image_url: string;
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
  display_name: string;
  profile_image_url: string;
}

export interface SubsResp {
  total: number;
  points: number;
  liste: SubEntry[];
}

export interface BroadcasterInfo {
  user_id: string;
  login: string;
  display_name: string;
  profile_image_url: string;
  broadcaster_type: string;
  description: string;
}

// ===== YouTube =====

export interface YoutubeChannelInfo {
  display_name: string;
  profile_image_url: string;
  subscriber_count: number;
  view_count: number;
  video_count: number;
  description: string;
}

export interface YoutubeMemberEntry {
  display_name: string;
  memberships_level: string;
}

export interface YoutubeMembersResp {
  total: number;
  liste: YoutubeMemberEntry[];
}

export const followers = writable<FollowersResp | null>(null);
export const subs = writable<SubsResp | null>(null);
export const viewers = writable<number | null>(null);
export const broadcaster = writable<BroadcasterInfo | null>(null);

// ===== YouTube stores =====
export const youtubeChannel = writable<YoutubeChannelInfo | null>(null);
export const youtubeMembers = writable<YoutubeMembersResp | null>(null);
export const youtubeViewers = writable<number | null>(null);

/// Erreur communauté. "need_reauth" = 403 (scopes manquants). null = OK.
export const communauteErreur = writable<string | null>(null);

// ===== Unfollows =====

export interface UnfollowEntry {
  user_id: string;
  login: string;
  /// Date de détection ISO 8601 (moment du démarrage StreamOS).
  date_unfollow: string;
  display_name: string;
  profile_image_url: string;
}

/// Historique des unfollows détectés au démarrage (lecture disque).
export const unfollows = writable<UnfollowEntry[] | null>(null);

/// Charge l'historique des unfollows depuis le disque.
export async function chargerUnfollows(): Promise<void> {
  try {
    const resp = await tauri.twitchCommunauteUnfollows();
    unfollows.set(resp);
  } catch {
  }
}

/// Charge les followers. Sur "need_reauth" → communauteErreur = "need_reauth".
export async function chargerFollowers(): Promise<void> {
  try {
    const resp = await tauri.twitchCommunauteFollowers();
    followers.set(resp);
    communauteErreur.set(null);
  } catch (e) {
    const msg = String(e);
    if (msg === "need_reauth") {
      communauteErreur.set("need_reauth");
    } else {
      communauteErreur.set(msg);
    }
  }
}

/// Charge les subs. Même gestion d'erreur.
export async function chargerSubs(): Promise<void> {
  try {
    const resp = await tauri.twitchCommunauteSubs();
    subs.set(resp);
    communauteErreur.set(null);
  } catch (e) {
    const msg = String(e);
    if (msg === "need_reauth") {
      communauteErreur.set("need_reauth");
    } else {
      communauteErreur.set(msg);
    }
  }
}

/// Charge les viewers live. None si hors-ligne.
export async function chargerViewers(): Promise<void> {
  try {
    const resp = await tauri.twitchCommunauteViewers();
    viewers.set(resp);
  } catch (e) {
    const msg = String(e);
    if (msg !== "need_reauth") {
      communauteErreur.set(msg);
    }
  }
}

/// Charge le broadcaster (display_name, avatar, type).
export async function chargerBroadcaster(): Promise<void> {
  try {
    const resp = await tauri.twitchBroadcaster();
    broadcaster.set(resp);
  } catch (e) {
    const msg = String(e);
    if (msg === "need_reauth") {
      communauteErreur.set("need_reauth");
    }
  }
}

/// Charge tout en parallèle (Promise.allSettled). Agrège les erreurs.
/// Charge Twitch ET YouTube si connectés. Inclut les unfollows (lecture disque).
export async function chargerCommunaute(): Promise<void> {
  await Promise.allSettled([
    chargerBroadcaster(),
    chargerFollowers(),
    chargerSubs(),
    chargerViewers(),
    chargerUnfollows(),
  ]);
}

// ===== YouTube =====

/// Charge les infos chaîne YouTube (display_name, avatar, stats).
export async function chargerYoutubeChannel(): Promise<void> {
  try {
    const resp = await tauri.youtubeCommunauteChannel();
    youtubeChannel.set(resp);
  } catch {
  }
}

/// Charge les members YouTube.
export async function chargerYoutubeMembers(): Promise<void> {
  try {
    const resp = await tauri.youtubeCommunauteMembers();
    youtubeMembers.set(resp);
  } catch {
  }
}

/// Charge les live viewers YouTube (null si pas live).
export async function chargerYoutubeViewers(): Promise<void> {
  try {
    const resp = await tauri.youtubeCommunauteViewers();
    youtubeViewers.set(resp);
  } catch {
  }
}

/// Charge toute la communauté YouTube en parallèle.
export async function chargerCommunauteYoutube(): Promise<void> {
  await Promise.allSettled([
    chargerYoutubeChannel(),
    chargerYoutubeMembers(),
    chargerYoutubeViewers(),
  ]);
}

/// Remet à zéro (après déconnexion).
export function resetCommunaute(): void {
  followers.set(null);
  subs.set(null);
  viewers.set(null);
  broadcaster.set(null);
  communauteErreur.set(null);
  // Unfollows (persistés sur disque, mais reset du store en mémoire)
  unfollows.set(null);
  // YouTube
  youtubeChannel.set(null);
  youtubeMembers.set(null);
  youtubeViewers.set(null);
}
