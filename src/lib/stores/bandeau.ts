// Store bandeau premier message : état config + listeners events Rust.
// Écoute "bandeau:etat" (pushed par Rust à chaque changement de config) +
// expose des actions (set config, stop, reset session, test).
import { writable } from "svelte/store";
import { listen } from "@tauri-apps/api/event";
import { tauri, type BandeauEtat } from "../tauri";

export const bandeauEtat = writable<BandeauEtat | null>(null);

let listenersBound = false;

/// Initialise le listener Tauri pour bandeau:etat. Idempotent.
/// À appeler au mount du dashboard (après initChat).
export async function initBandeau(): Promise<void> {
  if (listenersBound) return;
  listenersBound = true;

  await listen<BandeauEtat>("bandeau:etat", (e) => {
    bandeauEtat.set(e.payload);
  });

  console.log("[Bandeau] listeners bound");
}

/// Charge l'état initial du bandeau. À appeler après initBandeau.
export async function chargerBandeau(): Promise<void> {
  try {
    const etat = await tauri.bandeauEtat();
    bandeauEtat.set(etat);
  } catch (e) {
    console.error("[Bandeau] chargerBandeau ERR:", String(e));
  }
}

// ===== Actions (wrappers tauri → refresh état) =====

async function refresh(): Promise<void> {
  try {
    const etat = await tauri.bandeauEtat();
    bandeauEtat.set(etat);
  } catch (e) {
    console.error("[Bandeau] refresh ERR:", String(e));
  }
}

export async function bandeauSetActif(actif: boolean): Promise<void> {
  await tauri.bandeauSetConfig(actif, undefined, undefined);
  await refresh();
}

export async function bandeauSetDuree(ms: number): Promise<void> {
  await tauri.bandeauSetConfig(undefined, ms, undefined);
  await refresh();
}

export async function bandeauSetPosition(position: "bas" | "haut"): Promise<void> {
  await tauri.bandeauSetConfig(undefined, undefined, position);
  await refresh();
}

export async function bandeauStop(): Promise<void> {
  await tauri.bandeauStop();
}

export async function bandeauResetSession(): Promise<void> {
  await tauri.bandeauResetSession();
}

export async function bandeauTester(displayName: string, message: string): Promise<void> {
  await tauri.bandeauTester(displayName, message);
}

/// Remet à zéro (après déconnexion).
export function resetBandeau(): void {
  bandeauEtat.set(null);
}
