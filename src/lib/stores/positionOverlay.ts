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
//
// Le chrono 2 (deuxième chronomètre du pad numérique) a son PROPRE squelette
// dédié, indépendant du squelette partagé (position_chrono.json) :
// `positionChrono` + sauverPositionChrono — frame WS `chrono2-pos`, event
// Tauri `position-chrono:etat`.
//
// Le pop de validation des splits (speedrun) a AUSSI son squelette dédié
// (position_splitpop.json) : `positionSplitpop` + sauverPositionSplitpop —
// frame WS `splitpop-pos`, event `position-splitpop:etat`. Dédié car le pop
// arrive à chaque split et pouvait chevaucher les alertes du cadre partagé.
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

/// Défaut du squelette chrono 2 (miroir de defaut_chrono — haut-centre).
const DEFAUT_CHRONO: PositionOverlayConfig = {
  x: 760,
  y: 80,
  largeur: 400,
  hauteur: 120,
};

/// Défaut du squelette splitpop (miroir de defaut_splitpop — haut-centre,
/// reprend l'ancien défaut JS splitPopGeo).
const DEFAUT_SPLITPOP: PositionOverlayConfig = {
  x: 660,
  y: 60,
  largeur: 600,
  hauteur: 120,
};

export const positionOverlay = writable<PositionOverlayConfig>({ ...DEFAUT });

/// Squelette DÉDIÉ du chrono 2 (indépendant du squelette partagé).
export const positionChrono = writable<PositionOverlayConfig>({ ...DEFAUT_CHRONO });

/// Squelette DÉDIÉ du pop de validation des splits (speedrun).
export const positionSplitpop = writable<PositionOverlayConfig>({ ...DEFAUT_SPLITPOP });

let listenersBound = false;

/// Initialise les listeners Tauri pour position-overlay:etat. Idempotent.
/// À appeler au mount du dashboard (App.svelte).
export async function initPositionOverlay(): Promise<void> {
  if (listenersBound) return;
  listenersBound = true;

  await listen<PositionOverlayConfig>("position-overlay:etat", (e) => {
    positionOverlay.set(e.payload);
  });

  await listen<PositionOverlayConfig>("position-chrono:etat", (e) => {
    positionChrono.set(e.payload);
  });

  await listen<PositionOverlayConfig>("position-splitpop:etat", (e) => {
    positionSplitpop.set(e.payload);
  });

}

/// Charge la config initiale depuis Rust. À appeler après initPositionOverlay.
export async function chargerPositionOverlay(): Promise<void> {
  try {
    positionOverlay.set(await tauri.positionOverlayEtat());
    positionChrono.set(await tauri.positionChronoEtat());
    positionSplitpop.set(await tauri.positionSplitpopEtat());
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

/// Remplace le squelette chrono 2 (store local + persiste via Rust
/// position_chrono.json + push WS chrono2-pos — repositionnement live).
export async function sauverPositionChrono(
  cfg: PositionOverlayConfig,
): Promise<void> {
  positionChrono.set(cfg);
  try {
    await tauri.positionChronoSet(cfg);
  } catch {
  }
}

/// Remplace le squelette splitpop (store local + persiste via Rust
/// position_splitpop.json + push WS splitpop-pos — repositionnement live).
export async function sauverPositionSplitpop(
  cfg: PositionOverlayConfig,
): Promise<void> {
  positionSplitpop.set(cfg);
  try {
    await tauri.positionSplitpopSet(cfg);
  } catch {
  }
}

/// Remet à zéro (après déconnexion / reset).
export function resetPositionOverlay(): void {
  positionOverlay.set({ ...DEFAUT });
  positionChrono.set({ ...DEFAUT_CHRONO });
  positionSplitpop.set({ ...DEFAUT_SPLITPOP });
}
