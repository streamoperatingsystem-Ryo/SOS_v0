// Store modération Twitch (Helix lecture + écriture).
// Listes VIPs/modérateurs/bannis + actions ban/timeout/unban/VIP/mod/suppr message.
// Erreur "need_reauth" = 403 (scopes manquants) → banner reconnexion.
import { writable } from "svelte/store";
import { tauri } from "../tauri";

export interface VipEntry {
  user_id: string;
  login: string;
  display_name: string;
  profile_image_url: string;
}

export interface ModEntry {
  user_id: string;
  login: string;
  display_name: string;
  profile_image_url: string;
}

export interface BannedEntry {
  user_id: string;
  login: string;
  created_at: string;
  expires_at: string | null;
  reason: string;
  display_name: string;
  profile_image_url: string;
}

export interface ResolvedUser {
  user_id: string;
  login: string;
  display_name: string;
}

export const vips = writable<VipEntry[] | null>(null);
export const moderateurs = writable<ModEntry[] | null>(null);
export const bannis = writable<BannedEntry[] | null>(null);

/// Erreur modération. "need_reauth" = 403 (scopes manquants). null = OK.
export const moderationErreur = writable<string | null>(null);

/// Charge les VIPs.
export async function chargerVips(): Promise<void> {
  try {
    const resp = await tauri.twitchListerVips();
    vips.set(resp);
    moderationErreur.set(null);
  } catch (e) {
    const msg = String(e);
    if (msg === "need_reauth") moderationErreur.set("need_reauth");
    else moderationErreur.set(msg);
  }
}

/// Charge les modérateurs.
export async function chargerModerateurs(): Promise<void> {
  try {
    const resp = await tauri.twitchListerModerateurs();
    moderateurs.set(resp);
    moderationErreur.set(null);
  } catch (e) {
    const msg = String(e);
    if (msg === "need_reauth") moderationErreur.set("need_reauth");
    else moderationErreur.set(msg);
  }
}

/// Charge les bannis/timeout.
export async function chargerBannis(): Promise<void> {
  try {
    const resp = await tauri.twitchListerBannis();
    bannis.set(resp);
    moderationErreur.set(null);
  } catch (e) {
    const msg = String(e);
    if (msg === "need_reauth") moderationErreur.set("need_reauth");
    else moderationErreur.set(msg);
  }
}

/// Charge les 3 listes en parallèle.
export async function chargerToutMod(): Promise<void> {
  await Promise.allSettled([chargerVips(), chargerModerateurs(), chargerBannis()]);
}

/// Résout un login en user_id (recherche par pseudo).
export async function resoudreUser(login: string): Promise<ResolvedUser | null> {
  try {
    return await tauri.twitchResoudreUser(login);
  } catch (e) {
    const msg = String(e);
    if (msg === "need_reauth") moderationErreur.set("need_reauth");
    throw e;
  }
}

/// Bannir un utilisateur. duree = undefined → ban permanent, number → timeout.
export async function bannir(
  userId: string,
  raison: string,
  duree?: number
): Promise<void> {
  await tauri.twitchBannir(userId, raison, duree);
  await chargerBannis();
}

/// Débannir un utilisateur.
export async function debannir(userId: string): Promise<void> {
  await tauri.twitchDebannir(userId);
  await chargerBannis();
}

/// Ajouter un VIP.
export async function ajouterVip(userId: string): Promise<void> {
  await tauri.twitchAjouterVip(userId);
  await chargerVips();
}

/// Retirer un VIP.
export async function retirerVip(userId: string): Promise<void> {
  await tauri.twitchRetirerVip(userId);
  await chargerVips();
}

/// Ajouter un modérateur.
export async function ajouterModerateur(userId: string): Promise<void> {
  await tauri.twitchAjouterModerateur(userId);
  await chargerModerateurs();
}

/// Retirer un modérateur.
export async function retirerModerateur(userId: string): Promise<void> {
  await tauri.twitchRetirerModerateur(userId);
  await chargerModerateurs();
}

/// Supprimer un message de chat.
export async function supprimerMessage(messageId: string): Promise<void> {
  await tauri.twitchSupprimerMessage(messageId);
}

/// Vider le chat (commande IRC /clear).
export async function clearChat(): Promise<boolean> {
  return tauri.twitchEnvoyerCommandeChat("/clear");
}

/// Remet à zéro (après déconnexion).
export function resetModeration(): void {
  vips.set(null);
  moderateurs.set(null);
  bannis.set(null);
  moderationErreur.set(null);
}
