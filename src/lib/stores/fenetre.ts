// Store fenêtre principale : écran de démarrage + mode d'affichage (section
// Ergonomie de la Toolbar). Persisté dans fenetre.json côté Rust — appliqué
// au boot dans setup() et en direct via fenetreAppliquer (immédiat +
// persisté, même pattern que la barre de raccourcis).
// La liste des moniteurs est partagée : raccourcisMoniteurs (stores/raccourcis).
import { writable } from "svelte/store";
import { tauri, type ModeFenetre } from "../tauri";

/// Index du moniteur de démarrage choisi (dans available_monitors — fallback
/// primaire côté Rust si l'écran a été débranché).
export const fenetreMonitorIndex = writable<number>(0);

/// Mode d'affichage choisi : "fenetre" | "maximise" | "fullscreen".
export const fenetreMode = writable<ModeFenetre>("maximise");

/// Charge la préférence persistée (fenetre.json — défauts historiques si
/// absent : moniteur primaire + maximisé).
export async function initFenetre(): Promise<void> {
  try {
    const etat = await tauri.fenetreEtat();
    fenetreMonitorIndex.set(etat.monitor_index);
    fenetreMode.set(etat.mode);
  } catch (e) {
    console.error("initFenetre:", e);
  }
}

/// Applique écran + mode immédiatement sur la fenêtre main et persiste
/// fenetre.json (préférence de démarrage). Appelé par chaque changement du
/// sélecteur moniteur ou des boutons de mode de la section Ergonomie.
export async function appliquerFenetre(
  monitorIndex: number,
  mode: ModeFenetre
): Promise<void> {
  try {
    const etat = await tauri.fenetreAppliquer(monitorIndex, mode);
    fenetreMonitorIndex.set(etat.monitor_index);
    fenetreMode.set(etat.mode);
  } catch (e) {
    console.error("appliquerFenetre:", e);
  }
}
