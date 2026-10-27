// Store Emoji Arena côté dashboard. "!fight" tapé par un viewer → combat
// d'emojis 1v1 tour par tour côté diffusion :4321 (vs viewer ou BOT). La
// détection et la logique de jeu sont côté Rust (hooks chat + machine à
// états) — le dashboard gère la config, l'activation et les tests.
// L'état est poussé par l'event Tauri "fight:update" (état complet).
import { writable } from "svelte/store";
import { listen } from "@tauri-apps/api/event";
import { tauri, type FightEtat, type FightConfig } from "../tauri";

/// État complet : config + partie courante + cooldowns actifs.
/// null tant que fight_etat n'a jamais répondu (modal fermée au boot).
export const fightEtat = writable<FightEtat | null>(null);

let initialized = false;

/// Enregistre le listener "fight:update" UNE FOIS + charge l'état initial.
/// À appeler à l'ouverture de l'onglet Emoji Arena (idempotent).
export async function initFight(): Promise<void> {
  if (initialized) return;
  initialized = true;
  await listen<FightEtat>("fight:update", (e) => fightEtat.set(e.payload));
  await chargerFight();
}

/// Recharge l'état complet depuis Rust (état = source de vérité Rust).
export async function chargerFight(): Promise<void> {
  try {
    fightEtat.set(await tauri.fightEtat());
  } catch (e) {
    console.error("chargerFight:", e);
  }
}

/// Sauvegarde la config (le Rust normalise les noms + refuse les
/// collisions avec les commandes chat et la musique). Retourne true si OK.
export async function sauverConfigFight(cfg: FightConfig): Promise<boolean> {
  try {
    await tauri.fightSetConfig(cfg);
    return true;
  } catch (e) {
    console.error("sauverConfigFight:", e);
    return false;
  }
}

/// Test manuel : lance un combat factice « Streamy » vs BOT côté diffusion.
export async function testerFight(): Promise<boolean> {
  try {
    await tauri.fightTester();
    return true;
  } catch (e) {
    console.error("testerFight:", e);
    return false;
  }
}

/// Force l'arrêt du combat ou du lobby en cours.
export async function stopFight(): Promise<void> {
  try {
    await tauri.fightStop();
  } catch (e) {
    console.error("stopFight:", e);
  }
}

/// Prévisualise la carte d'aide sur la diffusion (bouton dashboard).
export async function previsualiserAideFight(): Promise<void> {
  try {
    await tauri.fightPrevisualiserAide();
  } catch (e) {
    console.error("previsualiserAideFight:", e);
  }
}
