// Store YouTube côté dashboard. Le chat polling vit côté Rust (tokio task).
// Le frontend ne fait qu'appeler tauri.youtubeConnecter() qui fait TOUT côté Rust :
// Device Code Flow → token → resolve channel → chat polling → forward messages.
import { tauri } from "../tauri";

/// Lance la connexion YouTube (token coffre OU Device Code Flow).
/// Sur succès : connexions.youtube = true (via event youtube:connecte).
export async function connecterYoutube(): Promise<void> {
  try {
    await tauri.youtubeConnecter();
  } catch (e) {
    console.error("connecterYoutube:", e);
  }
}

/// Déconnecte YouTube : arrête chat + révoque token + efface coffre.
export async function deconnecterYoutube(): Promise<void> {
  try {
    await tauri.youtubeDeconnecter();
  } catch (e) {
    console.error("deconnecterYoutube:", e);
  }
}

/// Annule le Device Code Flow YouTube en cours (bouton Annuler du modal).
export async function annulerYoutubeDeviceFlow(): Promise<void> {
  try {
    await tauri.youtubeAnnulerDeviceFlow();
  } catch (e) {
    console.error("annulerYoutubeDeviceFlow:", e);
  }
}

/// Démarre manuellement le chat polling YouTube Live (bouton "Chat live ON").
/// Si pas de live actif → le chat s'arrête immédiatement (event youtube:pas-de-live).
export async function demarrerChatYoutube(): Promise<void> {
  try {
    await tauri.youtubeDemarrerChat();
  } catch (e) {
    console.error("demarrerChatYoutube:", e);
  }
}

/// Arrête le chat polling YouTube Live sans déconnecter le compte (bouton "Chat live OFF").
export async function arreterChatYoutube(): Promise<void> {
  try {
    await tauri.youtubeArreterChat();
  } catch (e) {
    console.error("arreterChatYoutube:", e);
  }
}
