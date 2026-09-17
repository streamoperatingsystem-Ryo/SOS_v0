// Store commandes chat côté dashboard. Config par commande (persistée Rust,
// même moteur que les alertes : file FIFO + cooldowns + timer). La détection
// "!commande" est côté Rust (hooks chat) : le dashboard ne gère QUE la config
// et le test manuel.
import { writable } from "svelte/store";
import { tauri, type CommandeConfig } from "../tauri";

export const commandesStore = writable<CommandeConfig[]>([]);

/// Charge les commandes depuis Rust. À appeler à l'ouverture de la modale.
export async function chargerCommandes(): Promise<void> {
  try {
    commandesStore.set(await tauri.commandesEtat());
  } catch (e) {
    console.error("chargerCommandes:", e);
  }
}

/// Ajoute/remplace une commande (upsert par id) + persiste via Rust.
/// Retourne la config stockée (id généré si vide) ou null en cas d'erreur.
export async function sauverCommande(
  cfg: CommandeConfig
): Promise<CommandeConfig | null> {
  try {
    const stockee = await tauri.commandeSetConfig(cfg);
    commandesStore.update((liste) => {
      const i = liste.findIndex((c) => c.id === stockee.id);
      if (i === -1) return [...liste, stockee];
      return liste.map((c) => (c.id === stockee.id ? stockee : c));
    });
    return stockee;
  } catch (e) {
    console.error("sauverCommande:", e);
    return null;
  }
}

/// Supprime une commande (store local + persiste via Rust).
export async function supprimerCommande(id: string): Promise<void> {
  commandesStore.update((liste) => liste.filter((c) => c.id !== id));
  try {
    await tauri.commandeSupprimer(id);
  } catch (e) {
    console.error("supprimerCommande:", e);
  }
}

/// Test manuel : déclenche la commande côté diffusion (bypass cooldowns).
export async function testerCommande(id: string): Promise<void> {
  try {
    await tauri.commandeTester(id);
  } catch (e) {
    console.error("testerCommande:", e);
  }
}
