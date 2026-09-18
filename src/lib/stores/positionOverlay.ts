// Store "Squelette de position" unifié — source de vérité unique pour la
// position et la taille des overlays "clip de bienvenue" et "alertes" côté
// diffusion. Avant, welcome.ts et alertes.ts avaient chacun leur overlay
// config (deux réglages séparés dans la modale). Désormais, un seul réglage
// dans l'onglet "Squelette de position" pilote les deux overlays via ce store.
//
// Flux :
//   1. Modale (onglet "Squelette de position") → sauverPositionOverlay() →
//      tauri.positionOverlaySet() → Rust persiste position_overlay.json +
//      émet WS position-overlay-config (forwardé vers diffusion.html) +
//      émet event Tauri position-overlay:etat.
//   2. diffusion.html reçoit position-overlay-config → applique la position
//      au root welcome ET au root alerte (les deux overlays se repositionnent
//      en live).
//   3. Au moment d'un play welcome/alerte, Rust ré-émet welcome-clip-config /
//      alerte-config en lisant la position depuis le state unifié — garantit
//      que la position est appliquée même si diffusion reconnecte après un
//      changement.
import { writable } from "svelte/store";
import { listen } from "@tauri-apps/api/event";
import { tauri, type PositionOverlayConfig } from "../tauri";

/// Défaut (miroir de position_overlay.rs Default).
const DEFAUT: PositionOverlayConfig = {
  x: 1480,
  y: 580,
  largeur: 320,
  hauteur: 352,
};

export const positionOverlay = writable<PositionOverlayConfig>({ ...DEFAUT });

let listenersBound = false;

/// Initialise les listeners Tauri pour position-overlay:etat. Idempotent.
/// À appeler au mount du dashboard (App.svelte).
export async function initPositionOverlay(): Promise<void> {
  if (listenersBound) return;
  listenersBound = true;

  await listen<PositionOverlayConfig>("position-overlay:etat", (e) => {
    positionOverlay.set(e.payload);
  });

}

/// Charge la config initiale depuis Rust. À appeler après initPositionOverlay.
export async function chargerPositionOverlay(): Promise<void> {
  try {
    positionOverlay.set(await tauri.positionOverlayEtat());
  } catch {
  }
}

/// Remplace la config (store local + persiste via Rust + push WS diffusion).
export async function sauverPositionOverlay(
  cfg: PositionOverlayConfig,
): Promise<void> {
  positionOverlay.set(cfg);
  try {
    await tauri.positionOverlaySet(cfg);
  } catch {
  }
}

/// Remet à zéro (après déconnexion / reset).
export function resetPositionOverlay(): void {
  positionOverlay.set({ ...DEFAUT });
}
