// Store commandes chat côté dashboard. Config par commande (persistée Rust,
// même moteur que les alertes : file FIFO + cooldowns + timer). La détection
// "!commande" est côté Rust (hooks chat) : le dashboard ne gère QUE la config
// et le test manuel.
import { writable, get } from "svelte/store";
import { tauri, type CommandeConfig } from "../tauri";
import { t } from "./langue";

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

/// Exporte les commandes comme pack dossier portable sous le nom donné —
/// saisi dans une ligne inline de l'onglet (pas de prompt() natif).
/// Le dialog Rust choisit le dossier PARENT : <parent>/<nom_pack>/
/// commandes.json + medias/. Retourne true si exporté (false = annulé ou
/// refusé → l'UI garde sa ligne de saisie ouverte).
export async function exporterCommandes(nomPack: string): Promise<boolean> {
  if (nomPack.trim().length === 0) {
    alert(get(t)("interactions.commandes.nomPackVide"));
    return false;
  }
  try {
    return await tauri.commandeExporter(nomPack.trim());
  } catch (e) {
    alert(
      get(t)("interactions.commandes.exportRefuse", { erreur: String(e) })
    );
    return false;
  }
}

/// Importe un pack de commandes (dialog dossier). Les noms sans conflit sont
/// importés directement ; retourne packDir + conflits à trancher par l'UI
/// (écraser → ecraserCommandes). null = annulé, erreur ou rien à trancher.
export async function importerCommandes(): Promise<{
  packDir: string;
  conflits: CommandeConfig[];
} | null> {
  try {
    const res = await tauri.commandeImporter();
    if (res.annule) return null;
    if (res.reserves.length > 0) {
      alert(
        get(t)("interactions.commandes.importReserves", {
          noms: res.reserves.map((n) => `!${n}`).join(", "),
        })
      );
    }
    await chargerCommandes();
    if (!res.pack_dir || res.conflits.length === 0) return null;
    return { packDir: res.pack_dir, conflits: res.conflits };
  } catch (e) {
    alert(get(t)("commun.importRefuse", { erreur: String(e) }));
    return null;
  }
}

/// Phase conflits : les commandes acceptées par l'utilisateur remplacent les
/// existantes de même nom (médias copiés depuis le pack).
export async function ecraserCommandes(
  packDir: string,
  cmds: CommandeConfig[]
): Promise<void> {
  try {
    await tauri.commandeImporterEcraser(packDir, cmds);
    await chargerCommandes();
  } catch (e) {
    alert(get(t)("commun.importRefuse", { erreur: String(e) }));
  }
}
