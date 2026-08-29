// Store Kick côté dashboard. Le client WebSocket Pusher vit côté Rust
// (tokio-tungstenite avec Origin: https://kick.com — nécessaire car le webview
// est rejeté par CORS avec Origin: localhost:1420). Le frontend ne fait
// qu'appeler tauri.kickConnecter(slug) qui fait TOUT côté Rust :
// resolve slug → token → WS → subscribe → forward messages.
import { writable } from "svelte/store";
import { tauri } from "../tauri";

/// Erreur Kick (affichage discret). null = pas d'erreur.
export const kickErreur = writable<string | null>(null);

/// Connecte au chat Kick d'un canal (slug = nom du canal, ex: "xqc").
/// Rust fait TOUT : resolve slug → fetch token → WS Pusher → forward messages.
/// emit kick:connecte (→ frontend point vert) quand subscribed.
export async function connecterKick(slug: string): Promise<void> {
  kickErreur.set(null);
  try {
    await tauri.kickConnecter(slug);
  } catch (e) {
    kickErreur.set(String(e));
  }
}

/// Déconnecte Kick : arrête le WS côté Rust + emit kick:deconnecte.
export async function deconnecterKick(): Promise<void> {
  try {
    await tauri.kickDeconnecter();
  } catch (e) {
    console.error("deconnecterKick:", e);
  }
  kickErreur.set(null);
}

/// Lit le slug Kick sauvegardé (pour pré-remplir l'input si auto-resume échoue).
export async function lireSlugSauve(): Promise<string | null> {
  try {
    return await tauri.kickSlugCourant();
  } catch {
    return null;
  }
}
