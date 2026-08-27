// Store scène côté dashboard. Source UI live pendant le drag.
// invoke update_scene (save + snapshot :4321) seulement au create + pointerup.
import { writable, get } from "svelte/store";
import { tauri, type Scene, type Widget } from "../tauri";

export const sceneStore = writable<Scene>({ widgets: [] });
export const loadedStore = writable(false);
export const selectedIdStore = writable<string | null>(null);

/// Charge la scène initiale depuis Rust (config.json).
export async function loadScene(): Promise<void> {
  try {
    const scene = await tauri.getScene();
    sceneStore.set(scene);
  } catch (e) {
    console.error("loadScene:", e);
  } finally {
    loadedStore.set(true);
  }
}

/// Ajoute un widget par défaut (200×150, type media, z auto, id unique).
/// Commit immédiat → save config.json + snapshot WS.
export async function createWidget(): Promise<void> {
  const current = get(sceneStore);
  const z = current.widgets.reduce((m, w) => Math.max(m, w.z), -1) + 1;
  const w: Widget = {
    id: crypto.randomUUID(),
    type: "media",
    x: 100,
    y: 100,
    largeur: 200,
    hauteur: 150,
    z,
  };
  sceneStore.update((s) => ({ ...s, widgets: [...s.widgets, w] }));
  await commitScene();
}

/// Maj locale seule (pendant le drag). PAS d'invoke — UI fluide.
export function moveWidgetLocal(id: string, x: number, y: number): void {
  sceneStore.update((s) => ({
    ...s,
    widgets: s.widgets.map((w) => (w.id === id ? { ...w, x, y } : w)),
  }));
}

/// Pousse la scène vers Rust → save config.json + snapshot WS.
/// Appelé au create et au pointerup (fin du drag).
export async function commitScene(): Promise<void> {
  try {
    await tauri.updateScene(get(sceneStore));
  } catch (e) {
    console.error("commitScene:", e);
  }
}

/// Sélectionne (ou désélectionne si null) un widget.
export function selectWidget(id: string | null): void {
  selectedIdStore.set(id);
}

/// Importe une image pour le widget sélectionné. Met à jour le store local
/// avec le chemin relatif retourné par Rust. Affiche une alerte en cas de
/// refus (taille/format).
export async function importMedia(): Promise<void> {
  const id = get(selectedIdStore);
  if (!id) return;
  try {
    const rel = await tauri.importMedia(id);
    if (rel) {
      sceneStore.update((s) => ({
        ...s,
        widgets: s.widgets.map((w) =>
          w.id === id ? { ...w, media: rel } : w
        ),
      }));
    }
  } catch (e) {
    console.error("importMedia:", e);
    alert("Import refusé : " + e);
  }
}
