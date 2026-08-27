// Store scène côté dashboard. Source UI live pendant le drag.
// invoke update_scene (save + snapshot :4321) seulement au create + pointerup.
import { writable, get } from "svelte/store";
import { tauri, type Scene, type Widget } from "../tauri";

export const sceneStore = writable<Scene>({
  widgets: [],
  canvasW: 1920,
  canvasH: 1080,
});
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
    rotateX: 0,
    rotateY: 0,
    mediaFit: "ajuster",
  };
  sceneStore.update((s) => ({ ...s, widgets: [...s.widgets, w] }));
  await commitScene();
}

/// Clamp canvas : moitié du widget reste visible, widget jamais perdu.
/// x ∈ [-w/2 , canvasW - w/2], y ∈ [-h/2 , canvasH - h/2].
function clampWidget(w: Widget, canvasW: number, canvasH: number): Widget {
  const minX = -w.largeur / 2;
  const maxX = canvasW - w.largeur / 2;
  const minY = -w.hauteur / 2;
  const maxY = canvasH - w.hauteur / 2;
  const x = Math.min(Math.max(w.x, minX), maxX);
  const y = Math.min(Math.max(w.y, minY), maxY);
  return { ...w, x, y };
}

/// Maj locale seule (pendant le drag). PAS d'invoke — UI fluide.
/// Clamp canvas appliqué (moitié visible).
export function moveWidgetLocal(id: string, x: number, y: number): void {
  sceneStore.update((s) => {
    const canvasW = s.canvasW ?? 1920;
    const canvasH = s.canvasH ?? 1080;
    return {
      ...s,
      widgets: s.widgets.map((w) =>
        w.id === id ? clampWidget({ ...w, x, y }, canvasW, canvasH) : w
      ),
    };
  });
}

/// Maj locale des dims + position (pendant le resize). PAS d'invoke.
/// Mini 80×80, puis clamp canvas (moitié visible).
export function resizeWidgetLocal(
  id: string,
  x: number,
  y: number,
  largeur: number,
  hauteur: number
): void {
  sceneStore.update((s) => {
    const canvasW = s.canvasW ?? 1920;
    const canvasH = s.canvasH ?? 1080;
    return {
      ...s,
      widgets: s.widgets.map((w) =>
        w.id === id
          ? clampWidget(
              { ...w, x, y, largeur, hauteur },
              canvasW,
              canvasH
            )
          : w
      ),
    };
  });
}

/// Maj locale des rotateX/rotateY (pendant le drag boule gizmo). PAS d'invoke.
export function rotateWidgetLocal(id: string, rx: number, ry: number): void {
  sceneStore.update((s) => ({
    ...s,
    widgets: s.widgets.map((w) =>
      w.id === id ? { ...w, rotateX: rx, rotateY: ry } : w
    ),
  }));
}

/// Change le mode d'affichage média. Maj locale + commit immédiat
/// (changement discret, pas de drag).
export async function setMediaFit(id: string, fit: string): Promise<void> {
  sceneStore.update((s) => ({
    ...s,
    widgets: s.widgets.map((w) => (w.id === id ? { ...w, mediaFit: fit } : w)),
  }));
  await commitScene();
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

/// Déduit le kind ("image"|"video") d'un chemin relatif média.
const VIDEO_EXT = ["mp4", "webm"];
function kindFromMedia(rel: string): string {
  const ext = rel.split(".").pop()?.toLowerCase() ?? "";
  return VIDEO_EXT.includes(ext) ? "video" : "image";
}

/// Importe un média (image OU vidéo) pour le widget sélectionné. Met à jour
/// le store local avec le chemin relatif retourné par Rust + le kind déduit
/// de l'extension. Affiche une alerte en cas de refus (taille/format).
export async function importMedia(): Promise<void> {
  const id = get(selectedIdStore);
  if (!id) return;
  try {
    const rel = await tauri.importMedia(id);
    if (rel) {
      const kind = kindFromMedia(rel);
      sceneStore.update((s) => ({
        ...s,
        widgets: s.widgets.map((w) =>
          w.id === id ? { ...w, media: rel, kind } : w
        ),
      }));
    }
  } catch (e) {
    console.error("importMedia:", e);
    alert("Import refusé : " + e);
  }
}
