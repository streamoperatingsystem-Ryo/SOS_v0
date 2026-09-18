// Store scène côté dashboard. Source UI live pendant le drag.
// invoke update_scene (save + snapshot :4321) seulement au create + pointerup.
import { writable, get } from "svelte/store";
import { tauri, type Scene, type Widget, type CadreConfig, type MorphPoint } from "../tauri";
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
  bgOffsetX: 0,
  bgOffsetY: 0,
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
/// z calculé SANS les widgets caméra (plage réservée 5000+) → reste 0..N.
/// Commit immédiat → save config.json + snapshot WS.
export async function createWidget(): Promise<void> {
  const current = get(sceneStore);
  const z = current.widgets.filter((w) => w.type !== "camera").reduce((m, w) => Math.max(m, w.z), -1) + 1;
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
    mediaOffsetX: 0,
    mediaOffsetY: 0,
    mediaPaused: true,
    mediaTime: 0,
  };
  sceneStore.update((s) => ({ ...s, widgets: [...s.widgets, w] }));
  await commitScene();
}

/// Ajoute un widget chat (300×400, type chat, z auto, filtre unifié, police 16).
/// z calculé SANS les widgets caméra (plage réservée 5000+) → reste 0..N.
/// Pas de média, pas de trou. Commit immédiat → save + snapshot WS.
export async function createChatWidget(): Promise<void> {
  const current = get(sceneStore);
  const z = current.widgets.filter((w) => w.type !== "camera").reduce((m, w) => Math.max(m, w.z), -1) + 1;
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

/// Ajoute un widget input viewer (500×220, type input-viewer, z auto).
/// Affiche les entrées clavier/souris en temps réel côté diffusion (capture
/// globale démarrée/arrêtée par le bouton ON/OFF dans les options du widget).
/// Défaut : mode clavier AZERTY, couleur presse #8b5cf6.
/// Commit immédiat → save + snapshot WS.
export async function createInputViewerWidget(): Promise<void> {
  const current = get(sceneStore);
  const z = current.widgets.filter((w) => w.type !== "camera").reduce((m, w) => Math.max(m, w.z), -1) + 1;
  const w: Widget = {
    id: crypto.randomUUID(),
    type: "input-viewer",
    x: 100,
    y: 100,
    largeur: 500,
    hauteur: 220,
    z,
    rotateX: 0,
    rotateY: 0,
    inputMode: "clavier",
    inputLayout: "azerty",
    inputSkin: "xbox",
    inputCouleur: "#8b5cf6",
  };
  sceneStore.update((s) => ({ ...s, widgets: [...s.widgets, w] }));
  await commitScene();
}

/// Ajoute un widget speedrun splitter (300×400, type speedrun). Le timer
/// et les splits sont rendus dans diffusion.html via WS (speedrun-etat).
/// Les contrôles manuels + chargement .asl/.lss + settings ASL sont dans
/// le widget dashboard (WidgetSpeedrun.svelte).
export async function createSpeedrunWidget(): Promise<void> {
  const current = get(sceneStore);
  const z = current.widgets.filter((w) => w.type !== "camera").reduce((m, w) => Math.max(m, w.z), -1) + 1;
  const w: Widget = {
    id: crypto.randomUUID(),
    type: "speedrun",
    x: 100,
    y: 100,
    largeur: 300,
    hauteur: 400,
    z,
    rotateX: 0,
    rotateY: 0,
    speedrunSettings: { start: true, split: true, reset: true },
    srPolice: "Rajdhani",
    srTaillePolice: 27,
  };
  sceneStore.update((s) => ({ ...s, widgets: [...s.widgets, w] }));
  await commitScene();
}

/// Ajoute un widget caméra (320×240, type camera, z=5000 plage réservée).
/// SINGLETON : refuse si un widget caméra existe déjà. La caméra est rendue
/// par OBS : source dshow_input "SOS-Caméra" sous SOS-Diffusion, calée sur
/// le widget ; côté diffusion le widget est un TROU dans le canvas de fond.
/// Pas de média, pas de trou (le trou est implicite au rendu).
/// z=5000 → au-dessus des widgets normaux (0..N), sous les clips (9998/9999).
/// Commit immédiat → save + snapshot WS, puis camera_sync (non-fatal si OBS
/// offline : la source sera créée par scene_sync_captures au boot / openScene
/// / connexion OBS).
export async function createCameraWidget(): Promise<void> {
  const current = get(sceneStore);
  if (current.widgets.some((w) => w.type === "camera")) {
    return;
  }
  const w: Widget = {
    id: crypto.randomUUID(),
    type: "camera",
    x: 100,
    y: 100,
    largeur: 320,
    hauteur: 240,
    z: 5000,
    rotateX: 0,
    rotateY: 0,
    obsSource: "SOS-Caméra",
  };
  sceneStore.update((s) => ({ ...s, widgets: [...s.widgets, w] }));
  await commitScene();
  // Création de la source OBS "SOS-Caméra" (device par défaut d'OBS).
  // Non-fatal : OBS offline → sync différé (auto-guérison au connect).
  try {
    await tauri.cameraSync(
      get(obsHost), parseInt(get(obsPort), 10), get(obsPassword),
      null, w.x, w.y, w.largeur, w.hauteur
    );
  } catch {
  }
}

/// Change le device caméra d'un widget camera. Maj locale + commit, puis
/// camera_sync avec le device (SetInputSettings video_device_id sur l'input
/// "SOS-Caméra" existant — un seul input, jamais de doublon).
/// deviceId = FriendlyName PnP (= video_device_id dshow). null = device par
/// défaut du système.
export async function setCameraDevice(id: string, deviceId: string | null): Promise<void> {
  sceneStore.update((s) => ({
    ...s,
    widgets: s.widgets.map((w) =>
      w.id === id ? { ...w, cameraDeviceId: deviceId ?? undefined } : w
    ),
  }));
  await commitScene();
  const w = get(sceneStore).widgets.find((x) => x.id === id);
  if (!w) return;
  try {
    await tauri.cameraSync(
      get(obsHost), parseInt(get(obsPort), 10), get(obsPassword),
      deviceId, w.x, w.y, w.largeur, w.hauteur
    );
  } catch {
  }
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

/// Change la police Google Font + taille de base d'un widget speedrun.
/// police = id de police (voir POLICES_TITRE dans fonts.ts). taille = px (8-48).
/// Maj locale + commit. Toutes les tailles internes du splitter sont en em →
/// suit cette base (comme la taille de police du chat).
export async function setSpeedrunPolice(
  id: string,
  police: string,
  taille: number
): Promise<void> {
  const t = Math.max(8, Math.min(48, Math.round(taille)));
  sceneStore.update((s) => ({
    ...s,
    widgets: s.widgets.map((w) =>
      w.id === id ? { ...w, srPolice: police, srTaillePolice: t } : w
    ),
  }));
  await commitScene();
}

/// Change la config du titre d'un widget (merge partiel + commit).
/// Passer titre = "" ou undefined pour supprimer le titre.
export async function setWidgetTitre(
  id: string,
  patch: Partial<Pick<Widget, "titre" | "titrePosition" | "titrePolice" | "titreTaille" | "titreGras" | "titreItalique" | "titreSouligne" | "titreEspacement">>
): Promise<void> {
  sceneStore.update((s) => ({
    ...s,
    widgets: s.widgets.map((w) => (w.id === id ? { ...w, ...patch } : w)),
  }));
  await commitScene();
}

/// Change la config du titre du fond de l'application (merge partiel + commit).
/// Passer bgTitre = "" ou undefined pour supprimer le titre du fond.
export async function setBgTitre(
  patch: Partial<Pick<Scene, "bgTitre" | "bgTitrePosition" | "bgTitrePolice" | "bgTitreTaille" | "bgTitreGras" | "bgTitreItalique" | "bgTitreSouligne" | "bgTitreEspacement">>
): Promise<void> {
  sceneStore.update((s) => ({ ...s, ...patch }));
  await commitScene();
}

/// Change la config d'un widget input viewer (merge partiel + commit).
export async function setInputViewerConfig(
  id: string,
  patch: Partial<Pick<Widget, "inputMode" | "inputLayout" | "inputSkin" | "inputCouleur" | "inputMapping">>
): Promise<void> {
  sceneStore.update((s) => ({
    ...s,
    widgets: s.widgets.map((w) => (w.id === id ? { ...w, ...patch } : w)),
  }));
  await commitScene();
}

/// Change la config d'un widget speedrun (chemins ASL/LSS + settings ASL).
/// Merge partiel + commit. Les chemins permettent le rechargement auto au
/// montage du widget ; les settings sont aussi globaux (moteur unique).
export async function setSpeedrunConfig(
  id: string,
  patch: Partial<Pick<Widget, "speedrunCheminAsl" | "speedrunCheminLss" | "speedrunSettings">>
): Promise<void> {
  sceneStore.update((s) => ({
    ...s,
    widgets: s.widgets.map((w) => (w.id === id ? { ...w, ...patch } : w)),
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
      } catch {
        // OBS offline → on clear quand même côté scène (pas de crash).
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

/// Maj locale de l'offset X média (pendant le geste range). PAS d'invoke.
/// Clamp -2000 … 2000 (px).
export function setMediaOffsetXLocal(id: string, ox: number): void {
  const v = Math.max(-2000, Math.min(2000, ox));
  sceneStore.update((s) => ({
    ...s,
    widgets: s.widgets.map((w) => (w.id === id ? { ...w, mediaOffsetX: v } : w)),
  }));
}

/// Maj locale de l'offset Y média (pendant le geste range). PAS d'invoke.
/// Clamp -2000 … 2000 (px).
export function setMediaOffsetYLocal(id: string, oy: number): void {
  const v = Math.max(-2000, Math.min(2000, oy));
  sceneStore.update((s) => ({
    ...s,
    widgets: s.widgets.map((w) => (w.id === id ? { ...w, mediaOffsetY: v } : w)),
  }));
}

// ===== Effets visuels du média (luminosité/contraste/teinte/flou/pixel) =====
// Maj locale seule (pendant le geste range). PAS d'invoke — UI fluide.
// Clamp selon le type d'effet. Le commit se fait au pointerup (onchange).
function setMediaEffetLocal(id: string, champ: string, val: number, min: number, max: number): void {
  const v = Math.max(min, Math.min(max, val));
  sceneStore.update((s) => ({
    ...s,
    widgets: s.widgets.map((w) => (w.id === id ? { ...w, [champ]: v } : w)),
  }));
}
export function setMediaLumLocal(id: string, v: number): void { setMediaEffetLocal(id, "mediaLum", v, -100, 100); }
export function setMediaContrasteLocal(id: string, v: number): void { setMediaEffetLocal(id, "mediaContraste", v, -100, 100); }
export function setMediaTeinteLocal(id: string, v: number): void { setMediaEffetLocal(id, "mediaTeinte", v, 0, 360); }
export function setMediaFlouLocal(id: string, v: number): void { setMediaEffetLocal(id, "mediaFlou", v, 0, 20); }
export function setMediaPixelLocal(id: string, v: number): void { setMediaEffetLocal(id, "mediaPixel", v, 0, 50); }

/// Reset média : zoom 1, rotation 0, offsets 0, effets 0. Maj locale + commit immédiat.
export async function resetMedia(id: string): Promise<void> {
  sceneStore.update((s) => ({
    ...s,
    widgets: s.widgets.map((w) =>
      w.id === id ? { ...w, mediaZoom: 1, mediaRot: 0, mediaOffsetX: 0, mediaOffsetY: 0, mediaLum: 0, mediaContraste: 0, mediaTeinte: 0, mediaFlou: 0, mediaPixel: 0 } : w
    ),
  }));
  await commitScene();
}

/// Pousse la scène vers Rust → save config.json + snapshot WS.
/// Appelé au create et au pointerup (fin du drag).
/// Après updateScene : sync OBS des sources trou (one-shot reconnect) si ≥1
/// widget a obsSource. Erreur OBS → log discret, pas de crash.
export async function commitScene(): Promise<void> {
  // Guard : ne pas commiter tant que la scène n'est pas chargée depuis Rust.
  // Sinon on risque d'écraser la scène RAM (vraie scène chargée par boot_scenes)
  // avec la scène vide initiale du sceneStore → perte de données sur disque.
  if (!get(loadedStore)) {
    return;
  }
  try {
    await tauri.updateScene(get(sceneStore));
  } catch (e) {
    console.error("commitScene:", e);
    return;
  }
  // Sync OBS : sources trou + widget caméra liés aux widgets. One-shot
  // reconnect par commit. La caméra suit le widget (move/resize).
  const scene = get(sceneStore);
  const items = scene.widgets
    .filter((w) => (w.trou && w.obsSource) || w.type === "camera")
    .map((w) => {
      if (w.type === "camera") {
        // Caméra : source fixe "SOS-Caméra", fit remplir (cover OBS),
        // pas de zoom/rot/offset (pas de crop caméra en v1).
        return {
          sourceName: "SOS-Caméra",
          x: w.x,
          y: w.y,
          w: w.largeur,
          h: w.hauteur,
          fit: "remplir",
          zoom: 1,
          rot: 0,
          offsetX: 0,
          offsetY: 0,
        };
      }
      return {
        sourceName: w.obsSource!,
        x: w.x,
        y: w.y,
        w: w.largeur,
        h: w.hauteur,
        fit: w.mediaFit ?? "ajuster",
        zoom: w.mediaZoom ?? 1,
        rot: w.mediaRot ?? 0,
        offsetX: w.mediaOffsetX ?? 0,
        offsetY: w.mediaOffsetY ?? 0,
      };
    });
  if (items.length > 0) {
    try {
      await tauri.obsSyncTrous(
        get(obsHost), parseInt(get(obsPort), 10), get(obsPassword), items
      );
    } catch {
      // OBS offline → message discret, pas de crash. La scène reste OK.
    }
  }
}

/// ===== Morphing (bulge/pinch — widgets média + fond de scène) =====

/// Ajoute un morph au widget média sélectionné au point cliqué (coordonnées
/// normalisées du widget). Params (intensité/rayon/mode) viennent de la
/// sidebar (stores/morph). Commit → save + snapshot WS.
export async function ajouterMorphWidget(
  id: string,
  x: number,
  y: number,
  rayon: number,
  intensite: number,
  mode: "agrandir" | "retrecir"
): Promise<void> {
  const morph: MorphPoint = { x, y, rayon, intensite, mode };
  sceneStore.update((s) => ({
    ...s,
    widgets: s.widgets.map((w) =>
      w.id === id
        ? { ...w, morphs: [...(w.morphs ?? []), morph].slice(-20) }
        : w
    ),
  }));
  await commitScene();
}

/// Vide les morphs d'un widget (retour aux défauts = aucun morph).
export async function resetMorphsWidget(id: string): Promise<void> {
  sceneStore.update((s) => ({
    ...s,
    widgets: s.widgets.map((w) =>
      w.id === id ? { ...w, morphs: undefined } : w
    ),
  }));
  await commitScene();
}

/// Ajoute un morph au FOND de scène (coordonnées normalisées du canvas).
export async function ajouterMorphFond(
  x: number,
  y: number,
  rayon: number,
  intensite: number,
  mode: "agrandir" | "retrecir"
): Promise<void> {
  const morph: MorphPoint = { x, y, rayon, intensite, mode };
  sceneStore.update((s) => ({
    ...s,
    bgMorphs: [...(s.bgMorphs ?? []), morph].slice(-20),
  }));
  await commitScene();
}

/// Vide les morphs du fond de scène.
export async function resetMorphsFond(): Promise<void> {
  sceneStore.update((s) => ({ ...s, bgMorphs: undefined }));
  await commitScene();
}

/// Sélectionne (ou désélectionne si null) un widget.
export function selectWidget(id: string | null): void {
  selectedIdStore.set(id);
}

/// Supprime le widget sélectionné de la scène + commit (save + snapshot :4321).
/// Widget caméra → cache l'item OBS "SOS-Caméra" AVANT le remove (l'input est
/// conservé — la recréation le réutilisera). Non-fatal si OBS offline
/// (l'orphelin sera caché au prochain scene_sync_captures).
/// Le fichier média dans medias/ n'est PAS supprimé (un autre widget peut
/// le référencer ; nettoyage = plus tard). Désélectionne après suppression.
export async function deleteWidget(id: string): Promise<void> {
  const w = get(sceneStore).widgets.find((x) => x.id === id);
  if (w?.type === "camera") {
    try {
      await tauri.cameraHide(
        get(obsHost), parseInt(get(obsPort), 10), get(obsPassword)
      );
    } catch {
    }
  }
  sceneStore.update((s) => ({
    ...s,
    widgets: s.widgets.filter((x) => x.id !== id),
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
    const result = await tauri.importMedia(id);
    if (result) {
      const [rel, nom] = result;
      const kind = kindFromMedia(rel);
      sceneStore.update((s) => ({
        ...s,
        widgets: s.widgets.map((w) =>
          w.id === id ? { ...w, media: rel, kind, mediaNom: nom } : w
        ),
      }));
    }
  } catch (e) {
    alert("Import refusé : " + e);
  }
}

/// Retire le média d'un widget (widgets chat / input viewer / speedrun :
/// média de fond ; widgets média : retour au fond par défaut). Maj locale +
/// commit. Ne supprime PAS le fichier dans medias/ (un autre widget peut le
/// référencer — même règle que clearFond).
export async function clearWidgetMedia(id: string): Promise<void> {
  sceneStore.update((s) => ({
    ...s,
    widgets: s.widgets.map((w) =>
      w.id === id ? { ...w, media: undefined, kind: undefined, mediaNom: undefined } : w
    ),
  }));
  await commitScene();
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

/// Maj locale de l'offset X fond (pendant le geste range). PAS d'invoke.
/// Clamp -2000 … 2000 (px).
export function setBgOffsetXLocal(ox: number): void {
  const v = Math.max(-2000, Math.min(2000, ox));
  sceneStore.update((s) => ({ ...s, bgOffsetX: v }));
}

/// Maj locale de l'offset Y fond (pendant le geste range). PAS d'invoke.
/// Clamp -2000 … 2000 (px).
export function setBgOffsetYLocal(oy: number): void {
  const v = Math.max(-2000, Math.min(2000, oy));
  sceneStore.update((s) => ({ ...s, bgOffsetY: v }));
}

// ===== Effets visuels du fond (luminosité/contraste/teinte/flou/pixel) =====
// Maj locale seule (pendant le geste range). PAS d'invoke — UI fluide.
// Clamp selon le type d'effet. Le commit se fait au pointerup (onchange).
function setBgEffetLocal(champ: string, val: number, min: number, max: number): void {
  const v = Math.max(min, Math.min(max, val));
  sceneStore.update((s) => ({ ...s, [champ]: v }));
}
export function setBgLumLocal(v: number): void { setBgEffetLocal("bgLum", v, -100, 100); }
export function setBgContrasteLocal(v: number): void { setBgEffetLocal("bgContraste", v, -100, 100); }
export function setBgTeinteLocal(v: number): void { setBgEffetLocal("bgTeinte", v, 0, 360); }
export function setBgFlouLocal(v: number): void { setBgEffetLocal("bgFlou", v, 0, 20); }
export function setBgPixelLocal(v: number): void { setBgEffetLocal("bgPixel", v, 0, 50); }

/// Reset fond : zoom 1, rotation 0, offsets 0, effets 0. Maj locale + commit immédiat.
export async function resetFond(): Promise<void> {
  sceneStore.update((s) => ({ ...s, bgZoom: 1, bgRot: 0, bgOffsetX: 0, bgOffsetY: 0, bgLum: 0, bgContraste: 0, bgTeinte: 0, bgFlou: 0, bgPixel: 0 }));
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
  } catch {
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

// ===== Grille magnétique (snap entre widgets + canvas) =====
// Cibles figées au début du drag (snapshot) → pas de get(sceneStore) par frame.
// Seuil en px canvas (pas screen). Bypass si Alt enfoncé (free-drag).
// Les guides visuels (AlignmentGuides) restent affichés en parallèle.

export const SNAP_PX = 6;

type SnapSide = "left" | "center" | "right" | "top" | "bottom";
export type SnapTarget = { pos: number; side: SnapSide };

/// Snapshot des cibles de snap pour un drag donné.
/// Construit la liste des bords/centres des autres widgets + bords/centre canvas.
/// Appelé UNE FOIS au pointerdown (pas par frame).
export function buildSnapTargets(scene: Scene, draggedId: string): {
  xs: SnapTarget[];
  ys: SnapTarget[];
} {
  const cw = scene.canvasW ?? 1920;
  const ch = scene.canvasH ?? 1080;
  const xs: SnapTarget[] = [
    { pos: 0, side: "left" },
    { pos: cw / 2, side: "center" },
    { pos: cw, side: "right" },
  ];
  const ys: SnapTarget[] = [
    { pos: 0, side: "top" },
    { pos: ch / 2, side: "center" },
    { pos: ch, side: "bottom" },
  ];
  for (const o of scene.widgets) {
    if (o.id === draggedId) continue;
    xs.push({ pos: o.x, side: "left" });
    xs.push({ pos: o.x + o.largeur / 2, side: "center" });
    xs.push({ pos: o.x + o.largeur, side: "right" });
    ys.push({ pos: o.y, side: "top" });
    ys.push({ pos: o.y + o.hauteur / 2, side: "center" });
    ys.push({ pos: o.y + o.hauteur, side: "bottom" });
  }
  return { xs, ys };
}

/// Calcule le snap pour une position brute donnée.
/// Retourne la position snappée (ou la position brute si aucun snap).
export function computeSnap(
  rawX: number,
  rawY: number,
  w: number,
  h: number,
  targets: { xs: SnapTarget[]; ys: SnapTarget[] }
): { x: number; y: number } {
  // Points du widget déplacé (côté + position courante).
  const dragXs: SnapTarget[] = [
    { pos: rawX, side: "left" },
    { pos: rawX + w / 2, side: "center" },
    { pos: rawX + w, side: "right" },
  ];
  const dragYs: SnapTarget[] = [
    { pos: rawY, side: "top" },
    { pos: rawY + h / 2, side: "center" },
    { pos: rawY + h, side: "bottom" },
  ];

  let snapX: number | null = null;
  let snapY: number | null = null;
  let bestDX = SNAP_PX;
  let bestDY = SNAP_PX;

  // X : pour chaque point du widget déplacé, cherche la cible la plus proche.
  for (const d of dragXs) {
    for (const t of targets.xs) {
      const delta = t.pos - d.pos;
      const ad = Math.abs(delta);
      if (ad <= bestDX) {
        bestDX = ad;
        snapX = rawX + delta;
      }
    }
  }
  // Y : idem.
  for (const d of dragYs) {
    for (const t of targets.ys) {
      const delta = t.pos - d.pos;
      const ad = Math.abs(delta);
      if (ad <= bestDY) {
        bestDY = ad;
        snapY = rawY + delta;
      }
    }
  }

  return {
    x: snapX ?? rawX,
    y: snapY ?? rawY,
  };
}

// ===== Cadres SVG (widget + app) =====

const CADRE_DEFAUT: CadreConfig = {
  style: "carre",
  strokeWidth: 4,
  couleur: "#ffffff",
  couleurFin: "#000000",
  gradientAngle: 135,
  actif: false,
};

/// Met à jour le cadre des widgets (merge partiel + commit).
export async function mettreAJourCadreScene(patch: Partial<CadreConfig>): Promise<void> {
  sceneStore.update((s) => {
    const cadreActuel = s.cadreWidget ?? { ...CADRE_DEFAUT };
    return { ...s, cadreWidget: { ...cadreActuel, ...patch } };
  });
  await commitScene();
}

/// Met à jour le cadre de l'application (bord canvas, merge partiel + commit).
export async function mettreAJourCadreApp(patch: Partial<CadreConfig>): Promise<void> {
  sceneStore.update((s) => {
    const cadreActuel = s.cadreApp ?? { ...CADRE_DEFAUT };
    return { ...s, cadreApp: { ...cadreActuel, ...patch } };
  });
  await commitScene();
}
