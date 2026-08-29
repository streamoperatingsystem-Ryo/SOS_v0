// Store chat côté dashboard. Connexions 5 plateformes + messages + filtre.
// Listeners Tauri : chat:message (IRC), twitch:device/connecte/deconnecte/erreur.
// Pas de polling au boot — l'auto-resume Rust émet twitch:connecte si token valide.
import { writable } from "svelte/store";
import { listen } from "@tauri-apps/api/event";
import { tauri } from "../tauri";

export interface ChatMessage {
  plateforme: string;
  pseudo: string;
  texte: string;
  badges?: string;
}

export interface Connexions {
  twitch: boolean;
  youtube: boolean;
  kick: boolean;
  tiktok: boolean;
}

export interface DeviceInfo {
  user_code: string;
  verification_uri: string;
  expires_in: number;
}

export type ChatFiltre = "unifie" | "twitch" | "youtube" | "kick" | "tiktok";

const MAX_MESSAGES = 200;

export const connexions = writable<Connexions>({
  twitch: false,
  youtube: false,
  kick: false,
  tiktok: false,
});

export const chatMessages = writable<ChatMessage[]>([]);

/// Filtre chat éditable depuis la sidebar quand un widget chat est sélectionné.
/// Défaut "unifie". Sync 2-ways avec le widget chat sélectionné (scene.ts).
export const chatFiltre = writable<ChatFiltre>("unifie");

/// Infos Device Code Flow (pilotage modal). null = pas de flow en cours.
export const twitchDevice = writable<DeviceInfo | null>(null);

/// Erreur Twitch (affichage discret). null = pas d'erreur.
export const twitchErreur = writable<string | null>(null);

/// Login du compte Twitch connecté (null si déconnecté). Affiché dans le bandeau.
export const twitchLogin = writable<string | null>(null);

/// Infos Device Code Flow YouTube (pilotage modal). null = pas de flow en cours.
export const youtubeDevice = writable<DeviceInfo | null>(null);

/// Erreur YouTube (affichage discret). null = pas d'erreur.
export const youtubeErreur = writable<string | null>(null);

/// Erreur TikTok (affichage discret). null = pas d'erreur.
export const tiktokErreur = writable<string | null>(null);

/// Login de la chaîne YouTube connectée (null si déconnecté). Affiché dans le bandeau.
export const youtubeLogin = writable<string | null>(null);

/// Chat live YouTube actif (true = polling en cours, false = arrêté).
/// Contrôlé par le bouton "Chat live ON/OFF" dans la Toolbar.
/// Passe à false automatiquement quand le live se termine ou n'existe pas.
export const youtubeChatActif = writable<boolean>(false);

/// Slug du canal Kick connecté (null si déconnecté). Affiché dans le bandeau.
/// Kick n'émet pas le slug dans son event (il émet le channel Pusher), donc
/// on lit le slug persisté via kickSlugCourant() après connexion.
export const kickSlug = writable<string | null>(null);

/// Username du streamer TikTok suivi (null si déconnecté). Affiché dans le bandeau.
/// tiktok:connecte émet directement le username dans son payload.
export const tiktokUsername = writable<string | null>(null);

let initialized = false;

/// Initialise les listeners Tauri + lit l'état Twitch au boot.
/// À appeler une fois dans App.svelte onMount.
export async function initChat(): Promise<void> {
  if (initialized) return;
  initialized = true;

  // Message chat (IRC Twitch) → push + trim à 200.
  await listen<ChatMessage>("chat:message", (e) => {
    chatMessages.update((msgs) => {
      const next = [...msgs, e.payload];
      return next.length > MAX_MESSAGES ? next.slice(next.length - MAX_MESSAGES) : next;
    });
  });

  // Device Code Flow : infos à afficher dans le modal.
  await listen<DeviceInfo>("twitch:device", (e) => {
    twitchDevice.set(e.payload);
    twitchErreur.set(null);
  });

  // Connecté : point vert + login + fermer modal.
  await listen<string>("twitch:connecte", (e) => {
    connexions.update((c) => ({ ...c, twitch: true }));
    twitchLogin.set(e.payload);
    twitchDevice.set(null);
    twitchErreur.set(null);
  });

  // Déconnecté : point éteint + login effacé.
  await listen("twitch:deconnecte", () => {
    connexions.update((c) => ({ ...c, twitch: false }));
    twitchLogin.set(null);
  });

  // Erreur : message discret.
  await listen<string>("twitch:erreur", (e) => {
    twitchErreur.set(e.payload);
  });

  // Kick : connecté/déconnecté (le WS est côté Rust, ces events viennent
  // de Rust qui marque l'état). L'event émet le channel Pusher (pas le slug),
  // donc on lit le slug persisté pour l'afficher dans le bandeau.
  await listen<string>("kick:connecte", async () => {
    connexions.update((c) => ({ ...c, kick: true }));
    try {
      kickSlug.set(await tauri.kickSlugCourant());
    } catch {
      kickSlug.set(null);
    }
  });
  await listen("kick:deconnecte", () => {
    connexions.update((c) => ({ ...c, kick: false }));
    kickSlug.set(null);
  });

  // YouTube : Device Code Flow + connecté/déconnecté/erreur.
  await listen<DeviceInfo>("youtube:device", (e) => {
    youtubeDevice.set(e.payload);
    youtubeErreur.set(null);
  });
  await listen<string>("youtube:connecte", (e) => {
    connexions.update((c) => ({ ...c, youtube: true }));
    youtubeLogin.set(e.payload);
    youtubeDevice.set(null);
    youtubeErreur.set(null);
  });
  await listen("youtube:deconnecte", () => {
    connexions.update((c) => ({ ...c, youtube: false }));
    youtubeLogin.set(null);
    youtubeChatActif.set(false);
  });
  await listen<string>("youtube:erreur", (e) => {
    youtubeErreur.set(e.payload);
  });

  // Chat live YouTube : pas de live actif → le polling s'arrête, bouton OFF.
  await listen("youtube:pas-de-live", () => {
    youtubeChatActif.set(false);
  });

  // TikTok : connecté/déconnecté/erreur (le WS PirateTok est côté Rust).
  // tiktok:connecte émet le username du streamer dans son payload.
  await listen<string>("tiktok:connecte", (e) => {
    connexions.update((c) => ({ ...c, tiktok: true }));
    tiktokUsername.set(e.payload);
  });
  await listen("tiktok:deconnecte", () => {
    connexions.update((c) => ({ ...c, tiktok: false }));
    tiktokUsername.set(null);
  });
  await listen<string>("tiktok:erreur", (e) => {
    tiktokErreur.set(e.payload);
  });

  // Lire l'état initial (l'auto-resume Rust a pu déjà émettre twitch:connecte).
  // Ne pas écraser si déjà à jour : si twitch:connecte a déjà mis twitch=true,
  // retourner la même référence évite de re-déclencher le $effect → chargerCommunaute().
  try {
    const etat = await tauri.twitchEtat();
    connexions.update((c) => c.twitch === etat ? c : ({ ...c, twitch: etat }));
    if (etat) {
      const login = await tauri.twitchLoginCourant();
      twitchLogin.set(login);
    }
  } catch (e) {
    console.error("initChat: twitchEtat:", e);
  }

  // Lire l'état Kick initial (l'auto-resume Rust a pu déjà émettre kick:connecte).
  try {
    const kickOn = await tauri.kickEtat();
    connexions.update((c) => c.kick === kickOn ? c : ({ ...c, kick: kickOn }));
    if (kickOn) {
      kickSlug.set(await tauri.kickSlugCourant());
    }
  } catch (e) {
    console.error("initChat: kickEtat:", e);
  }

  // Lire l'état YouTube initial (l'auto-resume Rust a pu déjà émettre youtube:connecte).
  try {
    const ytOn = await tauri.youtubeEtat();
    connexions.update((c) => c.youtube === ytOn ? c : ({ ...c, youtube: ytOn }));
    if (ytOn) {
      const login = await tauri.youtubeLoginCourant();
      youtubeLogin.set(login);
    }
  } catch (e) {
    console.error("initChat: youtubeEtat:", e);
  }

  // Lire l'état TikTok initial (l'auto-resume Rust a pu déjà émettre tiktok:connecte).
  try {
    const ttOn = await tauri.tiktokEtat();
    connexions.update((c) => c.tiktok === ttOn ? c : ({ ...c, tiktok: ttOn }));
    if (ttOn) {
      tiktokUsername.set(await tauri.tiktokUsernameCourant());
    }
  } catch (e) {
    console.error("initChat: tiktokEtat:", e);
  }
}

/// Lance la connexion Twitch (token coffre OU Device Code Flow).
/// Sur succès (pas d'exception) : connexions.twitch = true tout de suite.
/// Pour le path token-coffre, twitchConnecter ne retourne qu'après IRC démarré.
/// Pour le path device flow, twitchConnecter retourne immédiatement (poll async)
/// → connexions.twitch reste false jusqu'à l'event twitch:connecte (correct).
export async function connecterTwitch(): Promise<void> {
  twitchErreur.set(null);
  try {
    await tauri.twitchConnecter();
    // Succès sans exception : si Rust a déjà connecté (token coffre), marquer.
    // L'event twitch:connecte confirmera ; twitchEtat() rattrape si manqué.
    const etat = await tauri.twitchEtat();
    connexions.update((c) => ({ ...c, twitch: etat }));
  } catch (e) {
    twitchErreur.set(String(e));
  }
}

/// Déconnecte Twitch (arrête IRC + efface token).
export async function deconnecterTwitch(): Promise<void> {
  try {
    await tauri.twitchDeconnecter();
  } catch (e) {
    console.error("deconnecterTwitch:", e);
  }
}

/// Annule le Device Code Flow en cours (bouton Annuler du modal).
export async function annulerDeviceFlow(): Promise<void> {
  try {
    await tauri.twitchAnnulerDeviceFlow();
  } catch (e) {
    console.error("annulerDeviceFlow:", e);
  }
  twitchDevice.set(null);
}

/// Force une reconnexion Twitch : révoque l'ancien token + efface le coffre +
/// lance un nouveau Device Code Flow avec les scopes étendus. Utilisé quand
/// les tokens existants n'ont pas les scopes communauté (Helix 403).
export async function reconnecterTwitch(): Promise<void> {
  twitchErreur.set(null);
  try {
    await tauri.twitchReconnecter();
  } catch (e) {
    twitchErreur.set(String(e));
  }
}
