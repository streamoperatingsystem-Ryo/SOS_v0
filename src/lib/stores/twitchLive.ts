// Store contrôles en direct Twitch (Helix — section « En direct » de la
// modale Modération). Zéro état persisté : chargerLive() relit Helix à
// l'ouverture de la section. Erreur "need_reauth" = 403 (scopes manquants)
// → banner reconnexion, JAMAIS de device flow auto.
//
// initTwitchLive() enregistre les listeners EventSub UNE FOIS au boot
// (App.svelte, à côté de initChat) : un message AutoMod retenu pendant
// que la modale est fermée ne doit pas être perdu.
import { writable } from "svelte/store";
import { listen } from "@tauri-apps/api/event";
import { tauri } from "../tauri";

export interface ChannelInfo {
  title: string;
  game_id: string;
  game_name: string;
  tags: string[];
}

export interface CategoryEntry {
  id: string;
  name: string;
  box_art_url: string;
}

export interface ChannelSearchEntry {
  id: string;
  login: string;
  display_name: string;
  is_live: boolean;
  thumbnail_url: string;
}

export interface ChatSettings {
  emote_mode: boolean;
  follower_mode: boolean;
  follower_mode_duration: number | null;
  slow_mode: boolean;
  slow_mode_wait_time: number | null;
  subscriber_mode: boolean;
}

/// Corps partiel PATCH /chat/settings (clés snake_case = format Helix).
export interface ChatSettingsPatch {
  emote_mode?: boolean;
  follower_mode?: boolean;
  follower_mode_duration?: number;
  slow_mode?: boolean;
  slow_mode_wait_time?: number;
  subscriber_mode?: boolean;
}

export interface UnbanRequestEntry {
  id: string;
  user_id: string;
  login: string;
  display_name: string;
  text: string;
  created_at: string;
}

/// Message retenu par AutoMod (EventSub automod.message.hold v1).
export interface AutomodEntry {
  msg_id: string;
  user_id: string;
  login: string;
  display_name: string;
  text: string;
  category: string;
  level: number | null;
  held_at: string;
}

/// Payload de l'event Tauri `twitch:automod` (emit twitch_eventsub.rs).
interface AutomodEvent {
  type: "hold" | "update";
  event: {
    message_id: string;
    user_id: string;
    user_login: string;
    user_name: string;
    message?: { text?: string };
    category?: string;
    level?: number;
    held_at?: string;
    /// automod.message.update : "Approved" | "Denied" | "Expired" | ...
    status?: string;
  };
}

export const channelInfo = writable<ChannelInfo | null>(null);
export const chatSettings = writable<ChatSettings | null>(null);
/// null = pas encore lu (GET shield_mode en échec ou pas encore ouvert).
export const shieldActif = writable<boolean | null>(null);
/// Raid en cours (état local — Helix n'expose pas de GET /raids).
export const raidEnCours = writable<boolean>(false);
export const unbanRequests = writable<UnbanRequestEntry[] | null>(null);
/// File AutoMod : messages retenus, dédupliqués par msg_id.
export const automodFile = writable<AutomodEntry[]>([]);

/// Erreur section « En direct ». "need_reauth" = 403 → banner reconnexion.
export const liveErreur = writable<string | null>(null);

function erreur(e: unknown): void {
  const msg = String(e);
  liveErreur.set(msg === "need_reauth" ? "need_reauth" : msg);
}

// ===== Chargement (paresseux — à l'ouverture de la section) =====

async function chargerChannelInfo(): Promise<void> {
  try {
    channelInfo.set(await tauri.twitchLiveChannelInfo());
  } catch (e) {
    erreur(e);
  }
}

async function chargerChatSettings(): Promise<void> {
  try {
    chatSettings.set(await tauri.twitchLiveChatSettings());
  } catch (e) {
    erreur(e);
  }
}

async function chargerShield(): Promise<void> {
  try {
    shieldActif.set(await tauri.twitchLiveShieldStatus());
  } catch (e) {
    erreur(e);
  }
}

async function chargerUnban(): Promise<void> {
  try {
    unbanRequests.set(await tauri.twitchLiveUnbanRequests());
  } catch (e) {
    erreur(e);
  }
}

/// Relit Helix : infos chaîne + modes de chat + Shield + unban requests.
/// Appelé à l'ouverture de la section « En direct » (pas de poll).
export async function chargerLive(): Promise<void> {
  liveErreur.set(null);
  await Promise.allSettled([
    chargerChannelInfo(),
    chargerChatSettings(),
    chargerShield(),
    chargerUnban(),
  ]);
}

// ===== Actions (relais des commandes twitch_live_*) =====

/// Modifier titre/catégorie/tags. Champs undefined = non envoyés à Helix.
export async function majChannel(opts: {
  title?: string;
  gameId?: string;
  tags?: string[];
}): Promise<void> {
  try {
    await tauri.twitchLiveMajChannel(opts);
    liveErreur.set(null);
    await chargerChannelInfo();
  } catch (e) {
    erreur(e);
    throw e;
  }
}

/// Recherche de catégorie (suggestions jeu).
export async function rechercherCategories(
  query: string
): Promise<CategoryEntry[]> {
  try {
    return await tauri.twitchLiveSearchCategories(query);
  } catch (e) {
    erreur(e);
    throw e;
  }
}

/// Infos d'un jeu par id : box_art_url quand la section s'ouvre avec un
/// game_id déjà connu (la recherche reste la source au choix manuel).
/// null si l'id n'existe plus côté Twitch.
export async function infosJeu(
  gameId: string
): Promise<CategoryEntry | null> {
  try {
    return await tauri.twitchLiveGameInfo(gameId);
  } catch (e) {
    erreur(e);
    throw e;
  }
}

/// Recherche de chaîne (suggestions raid).
export async function rechercherChannels(
  query: string
): Promise<ChannelSearchEntry[]> {
  try {
    return await tauri.twitchLiveSearchChannels(query);
  } catch (e) {
    erreur(e);
    throw e;
  }
}

/// Créer un marqueur de stream (description optionnelle).
export async function creerMarker(description?: string): Promise<void> {
  try {
    await tauri.twitchLiveMarker(description);
    liveErreur.set(null);
  } catch (e) {
    erreur(e);
    throw e;
  }
}

/// Lancer une pub (durées Helix : 30/60/90/120/150/180).
export async function lancerPub(duree: number): Promise<void> {
  try {
    await tauri.twitchLivePub(duree);
    liveErreur.set(null);
  } catch (e) {
    erreur(e);
    throw e;
  }
}

/// Lancer un raid vers un login.
export async function lancerRaid(login: string): Promise<void> {
  try {
    await tauri.twitchLiveRaidLancer(login);
    raidEnCours.set(true);
    liveErreur.set(null);
  } catch (e) {
    erreur(e);
    throw e;
  }
}

/// Annuler le raid en cours.
export async function annulerRaid(): Promise<void> {
  try {
    await tauri.twitchLiveRaidAnnuler();
    raidEnCours.set(false);
    liveErreur.set(null);
  } catch (e) {
    erreur(e);
    throw e;
  }
}

/// Modifier les modes de chat (PATCH partiel) puis relire l'état via GET.
export async function patchChatSettings(
  settings: ChatSettingsPatch
): Promise<void> {
  try {
    await tauri.twitchLiveChatSettingsMaj(settings);
    liveErreur.set(null);
    await chargerChatSettings();
  } catch (e) {
    erreur(e);
    throw e;
  }
}

/// Envoyer une annonce colorée (blue|green|orange|purple|primary).
export async function envoyerAnnonce(
  message: string,
  couleur: string
): Promise<void> {
  try {
    await tauri.twitchLiveAnnonce(message, couleur);
    liveErreur.set(null);
  } catch (e) {
    erreur(e);
    throw e;
  }
}

/// Activer/désactiver le Shield Mode. La confirmation avant activation
/// est gérée par l'UI (coupe les modes de chat et vide le chat).
export async function setShield(actif: boolean): Promise<void> {
  try {
    await tauri.twitchLiveShieldSet(actif);
    shieldActif.set(actif);
    liveErreur.set(null);
  } catch (e) {
    erreur(e);
    throw e;
  }
}

/// Approuver/refuser une demande d'unban, puis relire la liste.
export async function resoudreUnban(
  unbanRequestId: string,
  approuver: boolean
): Promise<void> {
  try {
    await tauri.twitchLiveUnbanResoudre(unbanRequestId, approuver);
    liveErreur.set(null);
    await chargerUnban();
  } catch (e) {
    erreur(e);
    throw e;
  }
}

/// Autoriser/refuser un message retenu par AutoMod. Retrait local immédiat
/// (l'event automod.message.update confirme — dédup par msg_id).
export async function resoudreAutomod(
  msgId: string,
  autoriser: boolean
): Promise<void> {
  try {
    await tauri.twitchLiveAutomodResoudre(msgId, autoriser);
    automodFile.update((file) => file.filter((m) => m.msg_id !== msgId));
    liveErreur.set(null);
  } catch (e) {
    erreur(e);
    throw e;
  }
}

/// Remet tout à zéro (déconnexion Twitch).
export function resetLive(): void {
  channelInfo.set(null);
  chatSettings.set(null);
  shieldActif.set(null);
  raidEnCours.set(false);
  unbanRequests.set(null);
  automodFile.set([]);
  liveErreur.set(null);
}

let initialized = false;

/// Enregistre les listeners EventSub UNE FOIS — à appeler au boot du
/// dashboard (App.svelte, à côté de initChat), PAS à l'ouverture de la
/// section : un message AutoMod retenu modale fermée ne serait pas perdu.
export async function initTwitchLive(): Promise<void> {
  if (initialized) return;
  initialized = true;

  // File AutoMod : hold → upsert par msg_id ; update → retrait si résolu
  // (approved/denied/expired — y compris fait hors de l'app : dashboard
  // Twitch, expiration), sinon simple mise à jour de l'entrée.
  await listen<AutomodEvent>("twitch:automod", (e) => {
    const { type, event } = e.payload;
    const msgId = event?.message_id;
    if (!msgId) return;
    automodFile.update((file) => {
      if (type === "update") {
        const st = (event.status ?? "").toLowerCase();
        if (st === "approved" || st === "denied" || st === "expired") {
          return file.filter((m) => m.msg_id !== msgId);
        }
      }
      const entree: AutomodEntry = {
        msg_id: msgId,
        user_id: event.user_id ?? "",
        login: event.user_login ?? "",
        display_name: event.user_name || event.user_login || "",
        text: event.message?.text ?? "",
        category: event.category ?? "",
        level: event.level ?? null,
        held_at: event.held_at ?? "",
      };
      const idx = file.findIndex((m) => m.msg_id === msgId);
      if (idx >= 0) {
        const next = file.slice();
        next[idx] = entree;
        return next;
      }
      return [...file, entree];
    });
  });

  // 403 sur la souscription AutoMod (vieux token sans le scope) → banner
  // need_reauth. Ne lance PAS le device flow — choix de l'utilisateur.
  await listen("twitch:need_reauth", () => {
    liveErreur.set("need_reauth");
  });

  // Déconnexion Twitch → file et état obsolètes → reset.
  await listen("twitch:deconnecte", () => {
    resetLive();
  });
}
