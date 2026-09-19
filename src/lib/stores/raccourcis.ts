// Store barre de raccourcis : fenêtre TOPMOST dockée à un bord d'écran.
// La fenêtre Rust (raccourcis.rs) n'a AUCUNE logique métier — les clics icônes
// arrivent ici via l'event "raccourcis:action" écouté dans App.svelte, qui
// dispatche vers les fonctions existantes des autres stores.
import { writable } from "svelte/store";
import { listen } from "@tauri-apps/api/event";
import { tauri, type BordRaccourcis, type RaccourcisMoniteur } from "../tauri";

/// La barre est affichée (fenêtre réellement ouverte — vérité Rust).
export const raccourcisVisible = writable<boolean>(false);

/// Bord d'écran courant (persisté dans raccourcis.json).
export const raccourcisBord = writable<BordRaccourcis>("droite");

/// Index du moniteur choisi (dans available_monitors — fallback primaire
/// côté Rust si l'index ne pointe plus sur rien après un débranchement).
export const raccourcisMonitorIndex = writable<number>(0);

/// Liste des moniteurs Windows (pour le sélecteur de la Toolbar).
export const raccourcisMoniteurs = writable<RaccourcisMoniteur[]>([]);

/// Charge l'état initial (config persistée + fenêtre ouverte) + la liste des
/// moniteurs + le listener "raccourcis-closed" (fermeture externe → la Toolbar
/// repasse en « Afficher »).
export async function initRaccourcis(): Promise<void> {
  try {
    const etat = await tauri.raccourcisEtat();
    raccourcisVisible.set(etat.visible);
    raccourcisBord.set(etat.bord);
    raccourcisMonitorIndex.set(etat.monitor_index);
  } catch (e) {
    console.error("initRaccourcis etat:", e);
  }
  try {
    raccourcisMoniteurs.set(await tauri.raccourcisMoniteurs());
  } catch (e) {
    console.error("initRaccourcis moniteurs:", e);
  }
  await listen("raccourcis-closed", () => raccourcisVisible.set(false));
}

/// Applique bord + moniteur : crée/montre la barre dockée et persiste.
/// Appelé par le bouton « Afficher » et par chaque changement bord/moniteur.
export async function appliquerRaccourcis(
  bord: BordRaccourcis,
  monitorIndex: number
): Promise<void> {
  try {
    const etat = await tauri.raccourcisAppliquer(bord, monitorIndex);
    raccourcisVisible.set(etat.visible);
    raccourcisBord.set(etat.bord);
    raccourcisMonitorIndex.set(etat.monitor_index);
  } catch (e) {
    console.error("appliquerRaccourcis:", e);
  }
}

/// Masque la barre (détruit la fenêtre) + persiste visible:false.
export async function masquerRaccourcis(): Promise<void> {
  try {
    await tauri.raccourcisMasquer();
  } catch (e) {
    console.error("masquerRaccourcis:", e);
  }
  raccourcisVisible.set(false);
}
