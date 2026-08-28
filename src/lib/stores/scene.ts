// Store scène côté dashboard. Source UI live pendant le drag.
// invoke update_scene (save + snapshot :4321) seulement au create + pointerup.
import { writable, get } from "svelte/store";
import { tauri, type Scene, type Widget } from "../tauri";
import { obsHost, obsPort, obsPassword } from "./obs";

export const sceneStore = writable<Scene>({
  widgets: [],
  canvasW: 1920,
  canvasH: 1080,
  bgMedia: "",
  bgKind: "image",
  bgFit: "remplir",
  bgZoom: 1,
  bgRot: 0,
  bgPaused: true,
  bgTime: 0,
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
    mediaZoom: 1,
    mediaRot: 0,
    mediaPaused: true,
    mediaTime: 0,
  };
  sceneStore.update((s) => ({ ...s, widgets: [...s.widgets, w] }));
  await commitScene();
}

/// Ajoute un widget chat (300×400, type chat, z auto, filtre unifié, police 16).
/// Pas de média, pas de trou. Commit immédiat → save + snapshot WS.
export async function createChatWidget(): Promise<void> {
  const current = get(sceneStore);
  const z = current.widgets.reduce((m, w) => Math.max(m, w.z), -1) + 1;
  const w: Widget = {
    id: crypto.randomUUID(),
    type: "chat",
    x: 100,
    y: 100,
    largeur: 300,
    hauteur: 400,
    z,
    rotateX: 0,
    rotateY: 0,
    chatFiltre: "unifie",
    taillePolice: 16,
  };
  sceneStore.update((s) => ({ ...s, widgets: [...s.widgets, w] }));
  await commitScene();
}

/// Change le filtre chat d'un widget. Maj locale + commit.
export async function setChatFiltre(id: string, filtre: string): Promise<void> {
  sceneStore.update((s) => ({
    ...s,
    widgets: s.widgets.map((w) =>
      w.id === id ? { ...w, chatFiltre: filtre } : w
    ),
  }));
  await commitScene();
}

/// Change la taille de police d'un widget chat. Maj locale + commit.
export async function setChatTaillePolice(id: string, px: number): Promise<void> {
  const p = Math.max(8, Math.min(48, Math.round(px)));
  sceneStore.update((s) => ({
    ...s,
    widgets: s.widgets.map((w) =>
      w.id === id ? { ...w, taillePolice: p } : w
    ),
  }));
  await commitScene();
}

/// Active/désactive le mode trou sur un widget. Maj locale + commit.
/// trou=true → :4321 média caché + rectangle masqué (fond + widgets dessous).
/// trou=false + obsSource set → supprime la source OBS d'abord, puis clear.
export async function setWidgetTrou(id: string, trou: boolean): Promise<void> {
  if (!trou) {
    // Désactivation : si une source OBS est liée, la supprimer avant de clear.
    const w = get(sceneStore).widgets.find((x) => x.id === id);
    if (w?.obsSource) {
      try {
        await tauri.obsDeleteTrouSource(
          get(obsHost), parseInt(get(obsPort), 10), get(obsPassword),
          w.obsSource
        );
      } catch (e) {
        // OBS offline → on clear quand même côté scène (pas de crash).
        console.warn("setWidgetTrou: suppression OBS échouée:", e);
      }
    }
    sceneStore.update((s) => ({
      ...s,
      widgets: s.widgets.map((x) =>
        x.id === id ? { ...x, trou: false, obsSource: undefined } : x
      ),
    }));
  } else {
    sceneStore.update((s) => ({
      ...s,
      widgets: s.widgets.map((x) => (x.id === id ? { ...x, trou } : x)),
    }));
  }
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

/// Maj locale du zoom média (pendant le geste range). PAS d'invoke.
/// Clamp 0.2 … 5.0.
export function setMediaZoomLocal(id: string, zoom: number): void {
  const z = Math.max(0.2, Math.min(5.0, zoom));
  sceneStore.update((s) => ({
    ...s,
    widgets: s.widgets.map((w) => (w.id === id ? { ...w, mediaZoom: z } : w)),
  }));
}

/// Maj locale de la rotation média (pendant le geste range). PAS d'invoke.
/// Clamp -180 … 180 (degrés).
export function setMediaRotLocal(id: string, rotDeg: number): void {
  const r = Math.max(-180, Math.min(180, rotDeg));
  sceneStore.update((s) => ({
    ...s,
    widgets: s.widgets.map((w) => (w.id === id ? { ...w, mediaRot: r } : w)),
  }));
}

/// Reset média : zoom 1, rotation 0. Maj locale + commit immédiat.
export async function resetMedia(id: string): Promise<void> {
  sceneStore.update((s) => ({
    ...s,
    widgets: s.widgets.map((w) =>
      w.id === id ? { ...w, mediaZoom: 1, mediaRot: 0 } : w
    ),
  }));
  await commitScene();
}

/// Pousse la scène vers Rust → save config.json + snapshot WS.
/// Appelé au create et au pointerup (fin du drag).
/// Après updateScene : sync OBS des sources trou (one-shot reconnect) si ≥1
/// widget a obsSource. Erreur OBS → log discret, pas de crash.
export async function commitScene(): Promise<void> {
  try {
    await tauri.updateScene(get(sceneStore));
  } catch (e) {
    console.error("commitScene:", e);
    return;
  }
  // Sync OBS : sources trou liées aux widgets. One-shot reconnect par commit.
  const scene = get(sceneStore);
  const items = scene.widgets
    .filter((w) => w.trou && w.obsSource)
    .map((w) => ({
      sourceName: w.obsSource!,
      x: w.x,
      y: w.y,
      w: w.largeur,
      h: w.hauteur,
      fit: w.mediaFit ?? "ajuster",
      zoom: w.mediaZoom ?? 1,
      rot: w.mediaRot ?? 0,
    }));
  if (items.length > 0) {
    try {
      await tauri.obsSyncTrous(
        get(obsHost), parseInt(get(obsPort), 10), get(obsPassword), items
      );
    } catch (e) {
      // OBS offline → message discret, pas de crash. La scène reste OK.
      console.warn("commitScene: sync OBS échouée:", e);
    }
  }
}

/// Sélectionne (ou désélectionne si null) un widget.
export function selectWidget(id: string | null): void {
  selectedIdStore.set(id);
}

/// Supprime le widget sélectionné de la scène + commit (save + snapshot :4321).
/// Le fichier média dans medias/ n'est PAS supprimé (un autre widget peut
/// le référencer ; nettoyage = plus tard). Désélectionne après suppression.
export async function deleteWidget(id: string): Promise<void> {
  sceneStore.update((s) => ({
    ...s,
    widgets: s.widgets.filter((w) => w.id !== id),
  }));
  selectedIdStore.set(null);
  await commitScene();
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

/// Importe un média (image OU vidéo) comme fond de scène. Met à jour le store
/// local (bgMedia + bgKind déduit de l'extension). Affiche une alerte en cas
/// de refus (taille/format). Mêmes limites que importMedia (helper commun Rust).
export async function importFond(): Promise<void> {
  try {
    const rel = await tauri.importFond();
    if (rel) {
      const kind = kindFromMedia(rel);
      sceneStore.update((s) => ({ ...s, bgMedia: rel, bgKind: kind }));
    }
  } catch (e) {
    console.error("importFond:", e);
    alert("Import refusé : " + e);
  }
}

/// Change le mode d'affichage du fond. Maj locale + commit immédiat
/// (changement discret, pas de drag).
export async function setBgFit(fit: string): Promise<void> {
  sceneStore.update((s) => ({ ...s, bgFit: fit }));
  await commitScene();
}

/// Maj locale du zoom fond (pendant le geste range). PAS d'invoke.
/// Clamp 0.2 … 5.0.
export function setBgZoomLocal(zoom: number): void {
  const z = Math.max(0.2, Math.min(5.0, zoom));
  sceneStore.update((s) => ({ ...s, bgZoom: z }));
}

/// Maj locale de la rotation fond (pendant le geste range). PAS d'invoke.
/// Clamp -180 … 180 (degrés).
export function setBgRotLocal(rotDeg: number): void {
  const r = Math.max(-180, Math.min(180, rotDeg));
  sceneStore.update((s) => ({ ...s, bgRot: r }));
}

/// Reset fond : zoom 1, rotation 0. Maj locale + commit immédiat.
export async function resetFond(): Promise<void> {
  sceneStore.update((s) => ({ ...s, bgZoom: 1, bgRot: 0 }));
  await commitScene();
}

/// Supprime le fond : retire la référence bgMedia (garde bgKind/bgFit/bgZoom/
/// bgRot). Maj locale + commit. Ne supprime PAS le fichier dans medias/.
export async function clearFond(): Promise<void> {
  sceneStore.update((s) => ({ ...s, bgMedia: "" }));
  await commitScene();
}

// ===== Barre lecteur (pilote :4321 via snapshot, dashboard figé) =====

/// Change l'état lecture/pause d'un widget vidéo → commit (snapshot :4321).
/// Dashboard : ignore mediaPaused pour la lecture (toujours pause, vignette).
export async function setWidgetMediaPaused(id: string, paused: boolean): Promise<void> {
  sceneStore.update((s) => ({
    ...s,
    widgets: s.widgets.map((w) =>
      w.id === id ? { ...w, mediaPaused: paused } : w
    ),
  }));
  await commitScene();
}

/// Change la position (seek) d'un widget vidéo → commit (snapshot :4321).
/// Dashboard : applique mediaTime une fois pour la vignette (frame figée).
export async function setWidgetMediaTime(id: string, time: number): Promise<void> {
  const t = Math.max(0, time);
  sceneStore.update((s) => ({
    ...s,
    widgets: s.widgets.map((w) =>
      w.id === id ? { ...w, mediaTime: t } : w
    ),
  }));
  await commitScene();
}

/// Change l'état lecture/pause du fond vidéo → commit (snapshot :4321).
export async function setBgPaused(paused: boolean): Promise<void> {
  sceneStore.update((s) => ({ ...s, bgPaused: paused }));
  await commitScene();
}

/// Change la position (seek) du fond vidéo → commit (snapshot :4321).
export async function setBgTime(time: number): Promise<void> {
  const t = Math.max(0, time);
  sceneStore.update((s) => ({ ...s, bgTime: t }));
  await commitScene();
}

// ===== Sources OBS "trou" (Lot 2) =====

/// Crée une source OBS de capture sous SOS-Diffusion, calée sur le widget.
/// Met à jour le store (obsSource) + commit (qui resync OBS). Retourne le nom.
export async function createObsTrouSource(
  id: string,
  kind: "camera" | "window" | "game",
  target: string | null
): Promise<string> {
  const w = get(sceneStore).widgets.find((x) => x.id === id);
  if (!w) throw new Error("Widget introuvable");
  const sourceName = "SOS-Trou-" + id.slice(0, 8);
  const name = await tauri.obsCreateTrouSource(
    get(obsHost), parseInt(get(obsPort), 10), get(obsPassword),
    sourceName, kind, target, w.x, w.y, w.largeur, w.hauteur
  );
  sceneStore.update((s) => ({
    ...s,
    widgets: s.widgets.map((x) =>
      x.id === id ? { ...x, obsSource: name } : x
    ),
  }));
  await commitScene();
  return name;
}

/// Supprime la source OBS liée à un widget trou + clear obsSource + commit.
export async function deleteObsTrouSource(id: string): Promise<void> {
  const w = get(sceneStore).widgets.find((x) => x.id === id);
  if (!w?.obsSource) return;
  try {
    await tauri.obsDeleteTrouSource(
      get(obsHost), parseInt(get(obsPort), 10), get(obsPassword),
      w.obsSource
    );
  } catch (e) {
    console.warn("deleteObsTrouSource: suppression OBS échouée:", e);
  }
  sceneStore.update((s) => ({
    ...s,
    widgets: s.widgets.map((x) =>
      x.id === id ? { ...x, obsSource: undefined } : x
    ),
  }));
  await commitScene();
}

/// Lie une source OBS existante au widget trou (pas de création, juste
/// transform + reorder). Met à jour le store (obsSource) + commit.
export async function linkExistingObsSource(
  id: string,
  sourceName: string
): Promise<string> {
  const w = get(sceneStore).widgets.find((x) => x.id === id);
  if (!w) throw new Error("Widget introuvable");
  const name = await tauri.obsLinkExistingSource(
    get(obsHost), parseInt(get(obsPort), 10), get(obsPassword),
    sourceName, w.x, w.y, w.largeur, w.hauteur
  );
  sceneStore.update((s) => ({
    ...s,
    widgets: s.widgets.map((x) =>
      x.id === id ? { ...x, obsSource: name } : x
    ),
  }));
  await commitScene();
  return name;
}

/// Crée une source OBS de capture depuis une cible PC (caméra/fenêtre/jeu)
/// dans la scène contenant SOS-Diffusion. Nom source = SOS-Trou-<id8>.
/// Si existe déjà → SetInputSettings + transform (pas de doublon).
/// Met à jour le store (obsSource) + commit.
export async function createObsTrouFromPc(
  id: string,
  kind: "camera" | "window" | "game",
  target: string | null
): Promise<string> {
  const w = get(sceneStore).widgets.find((x) => x.id === id);
  if (!w) throw new Error("Widget introuvable");
  const sourceName = "SOS-Trou-" + id.slice(0, 8);
  const name = await tauri.obsCreateTrouFromPc(
    get(obsHost), parseInt(get(obsPort), 10), get(obsPassword),
    sourceName, kind, target, w.x, w.y, w.largeur, w.hauteur
  );
  sceneStore.update((s) => ({
    ...s,
    widgets: s.widgets.map((x) =>
      x.id === id ? { ...x, obsSource: name } : x
    ),
  }));
  await commitScene();
  return name;
}
