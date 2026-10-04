// Store presets de scènes embarqués (modèles prêts à l'emploi).
// Galerie pour le user lambda : liste les métadonnées (chargerPresets) et
// applique un preset — en création de scène (nouvelle scène, l'existante
// jamais touchée) ou à l'onboarding (initialise la scène de départ vide).
import { writable, get } from "svelte/store";
import { tauri, type PresetInfo } from "../tauri";
import { loadScenesIndex, loadCurrentScene } from "./scenes";
import { loadScene, selectWidget, syncCapturesOBS } from "./scene";
import { t } from "./langue";

export const presetsStore = writable<PresetInfo[]>([]);

/// Charge la liste des presets embarqués (une fois au boot / à l'ouverture).
export async function chargerPresets(): Promise<void> {
  try {
    const liste = await tauri.presetsLister();
    presetsStore.set(liste);
  } catch (e) {
    console.error("chargerPresets:", e);
  }
}

/// Recharge l'UI après application d'un preset : index + courante + scène
/// RAM, puis sync captures OBS (widgets caméra — même chaîne que openScene).
async function rechargerApresPreset(): Promise<void> {
  await loadScenesIndex();
  await loadCurrentScene();
  await loadScene();
  selectWidget(null);
  await syncCapturesOBS();
}

/// Crée une NOUVELLE scène depuis un preset (bouton « + Scène » → galerie).
/// L'existante n'est jamais touchée. Retourne true si créée.
export async function creerSceneDepuisPreset(presetId: string, nom: string): Promise<boolean> {
  try {
    await tauri.sceneCreerDepuisPreset(nom, presetId);
    await rechargerApresPreset();
    return true;
  } catch (e) {
    alert(get(t)("presets.creationRefusee", { erreur: String(e) }));
    return false;
  }
}

/// ONBOARDING : initialise la scène de départ (courante, vide au 1er boot)
/// depuis un preset + marque l'onboarding comme fait. Retourne true si OK.
export async function initialiserSceneDepuisPreset(presetId: string): Promise<boolean> {
  try {
    await tauri.sceneInitialiserPreset(presetId);
    await rechargerApresPreset();
    await tauri.onboardingMarquerFait();
    return true;
  } catch (e) {
    alert(get(t)("presets.initRefusee", { erreur: String(e) }));
    return false;
  }
}
