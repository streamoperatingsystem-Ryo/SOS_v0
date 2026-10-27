// Store Diagnostic (consommation) — polling 1 Hz de `conso_etat` UNIQUEMENT
// pendant que la modale Diagnostic est ouverte (ConsoModal montée →
// demarrerPollingConso, démontée → arreterPollingConso). Aucun event stream :
// le backend échantillonne déjà à 1 Hz, le frontend lit l'historique complet
// et le ré-affiche (les nouvelles secondes arrivent naturellement).
import { writable } from "svelte/store";
import { tauri, type BoiteNoireEtat, type ConsoEtat } from "../tauri";

/// Dernier état connu (historique roulant côté Rust, ≤300 pts ≈ 5 min).
export const consoEtat = writable<ConsoEtat | null>(null);

/// true quand le dernier invoke a échoué (backend joignable ?).
export const consoErreur = writable(false);

/// Journal PERSISTANT (boîte noire) : 1 point / 5 s écrit sur disque, il
/// survit à un gel ou à une coupure. Lu à la demande — jamais pollé (le
/// fichier fait jusqu'à 2 Mio).
export const boiteNoire = writable<BoiteNoireEtat | null>(null);

/// Charge les `limite` derniers points du journal persistant (1 h par défaut).
export async function chargerBoiteNoire(limite = 720): Promise<void> {
  try {
    boiteNoire.set(await tauri.boiteNoireEtat(limite));
  } catch {
    // Non-fatal : la section affiche « aucun point enregistré ».
    boiteNoire.set(null);
  }
}

let timer: ReturnType<typeof setInterval> | null = null;

async function tick() {
  try {
    consoEtat.set(await tauri.consoEtat());
    consoErreur.set(false);
  } catch {
    // Non-fatal : la modale affiche l'état « mesure indisponible ».
    consoErreur.set(true);
  }
}

/// Démarre le polling 1 Hz (idempotent). Premier tick immédiat.
export function demarrerPollingConso(): void {
  if (timer !== null) return;
  void tick();
  timer = setInterval(tick, 1000);
}

/// Arrête le polling (fermeture de la modale). Idempotent.
export function arreterPollingConso(): void {
  if (timer !== null) {
    clearInterval(timer);
    timer = null;
  }
}
