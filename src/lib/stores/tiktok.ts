// Store TikTok côté dashboard. Le client WebSocket PirateTok vit côté Rust
// (reverse engineering du protocole Webcast TikTok). Le frontend ne fait
// qu'appeler tauri.tiktokConnecter(username) qui fait TOUT côté Rust :
// resolve room ID → WebSocket → forward messages.
import { writable } from "svelte/store";
import { tauri } from "../tauri";

/// Erreur TikTok (affichage discret). null = pas d'erreur.
export const tiktokErreur = writable<string | null>(null);

/// Connecte au chat TikTok Live d'un streamer (username sans @).
/// Rust fait TOUT : resolve room ID → WebSocket PirateTok → forward messages.
/// emit tiktok:connecte (→ frontend point vert) quand connecté.
export async function connecterTiktok(username: string): Promise<void> {
  tiktokErreur.set(null);
  try {
    await tauri.tiktokConnecter(username);
  } catch (e) {
    tiktokErreur.set(String(e));
  }
}

/// Déconnecte TikTok : arrête le WS côté Rust + emit tiktok:deconnecte.
export async function deconnecterTiktok(): Promise<void> {
  try {
    await tauri.tiktokDeconnecter();
  } catch (e) {
    console.error("deconnecterTiktok:", e);
  }
  tiktokErreur.set(null);
}

/// Lit le username TikTok sauvegardé (pour pré-remplir l'input si auto-resume échoue).
export async function lireUsernameSauve(): Promise<string | null> {
  try {
    return await tauri.tiktokUsernameCourant();
  } catch {
    return null;
  }
}
