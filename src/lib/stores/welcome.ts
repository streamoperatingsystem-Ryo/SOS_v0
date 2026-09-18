// Store clips de bienvenue : état queue + registre + listeners events Rust.
// Écoute "welcome:etat" (pushed par Rust à chaque changement) + expose
// des actions (stop, skip, retirer, remonter, descendre, vider, reset).
import { writable } from "svelte/store";
import { listen } from "@tauri-apps/api/event";
import { tauri, type WelcomeQueueEtat, type WelcomeViewerConfig, type WelcomeOverlayConfig } from "../tauri";

export const welcomeEtat = writable<WelcomeQueueEtat | null>(null);
export const welcomeRegistre = writable<[string, WelcomeViewerConfig][]>([]);
export const welcomeAttributionProgress = writable<{
  succes: number;
  echecs: number;
  login: string;
} | null>(null);

let listenersBound = false;

/// Initialise les listeners Tauri pour welcome:etat + welcome:attribution-progress.
/// Idempotent (peut être appelé plusieurs fois). À appeler au mount du dashboard.
export async function initWelcome(): Promise<void> {
  if (listenersBound) return;
  listenersBound = true;

  await listen<WelcomeQueueEtat>("welcome:etat", (e) => {
    welcomeEtat.set(e.payload);
  });

  await listen<{ succes: number; echecs: number; login: string }>(
    "welcome:attribution-progress",
    (e) => {
      welcomeAttributionProgress.set(e.payload);
    }
  );

}

/// Charge l'état initial de la queue + registre. À appeler après initWelcome.
export async function chargerWelcome(): Promise<void> {
  try {
    const etat = await tauri.welcomeEtat();
    welcomeEtat.set(etat);
  } catch {
  }
  try {
    const reg = await tauri.welcomeRegistreTwitch();
    welcomeRegistre.set(reg);
  } catch {
  }
}

// ===== Actions (wrappers tauri → refresh état) =====

async function refresh(): Promise<void> {
  try {
    const etat = await tauri.welcomeEtat();
    welcomeEtat.set(etat);
  } catch {
  }
}

export async function welcomeStop(): Promise<void> {
  await tauri.welcomeStop();
  await refresh();
}

export async function welcomeSkip(): Promise<void> {
  await tauri.welcomeSkip();
  await refresh();
}

export async function welcomeRetirer(id: string): Promise<void> {
  await tauri.welcomeRetirer(id);
  await refresh();
}

export async function welcomeRemonter(id: string): Promise<void> {
  await tauri.welcomeRemonter(id);
  await refresh();
}

export async function welcomeDescendre(id: string): Promise<void> {
  await tauri.welcomeDescendre(id);
  await refresh();
}

export async function welcomeVider(): Promise<void> {
  await tauri.welcomeVider();
  await refresh();
}

export async function welcomeResetSession(): Promise<void> {
  await tauri.welcomeResetSession();
}

export async function welcomeSetConfigGlobaleActif(actif: boolean): Promise<void> {
  await tauri.welcomeConfigGlobaleActif(actif);
  await refresh();
}

export async function welcomeSetOverlayConfig(config: WelcomeOverlayConfig): Promise<void> {
  await tauri.welcomeSetOverlayConfig(config);
  await refresh();
}

/// Test manuel : lance un clip côté diffusion (bypass queue). Résout MP4 +
/// émet welcome-clip-play. Utilisé pour tester un clip au clic dans la modale.
export async function welcomeTesterClip(
  clipId: string,
  clipTitre: string,
  clipDureeMs: number,
  displayName: string,
): Promise<void> {
  await tauri.welcomeTesterClip(clipId, clipTitre, clipDureeMs, displayName);
  await refresh();
}

export async function welcomeSetDureeAffichage(ms: number): Promise<void> {
  await tauri.welcomeSetDureeAffichage(ms);
  await refresh();
}

export async function welcomeSauverViewer(
  login: string,
  config: WelcomeViewerConfig
): Promise<void> {
  await tauri.welcomeSauverViewerTwitch(login, config);
  // Recharger le registre.
  try {
    const reg = await tauri.welcomeRegistreTwitch();
    welcomeRegistre.set(reg);
  } catch {
  }
}

export async function welcomeSupprimerViewer(login: string): Promise<void> {
  await tauri.welcomeSupprimerViewerTwitch(login);
  try {
    const reg = await tauri.welcomeRegistreTwitch();
    welcomeRegistre.set(reg);
  } catch {
  }
}

/// Attribution automatique : pour chaque follower, fetch clips → clip aléatoire
/// → résout MP4 → sauve config. Progress via welcomeAttributionProgress store.
/// Retourne { succes, echecs }.
export async function welcomeAttribuerAuto(
  followers: { login: string; user_id: string; display_name?: string }[]
): Promise<{ succes: number; echecs: number }> {
  const result = await tauri.welcomeAttribuerAuto(followers);
  // Recharger le registre après attribution.
  try {
    const reg = await tauri.welcomeRegistreTwitch();
    welcomeRegistre.set(reg);
  } catch {
  }
  welcomeAttributionProgress.set(null);
  return result;
}

/// Remet à zéro (après déconnexion).
export function resetWelcome(): void {
  welcomeEtat.set(null);
  welcomeRegistre.set([]);
  welcomeAttributionProgress.set(null);
}
