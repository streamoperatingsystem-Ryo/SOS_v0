// Store scène côté dashboard. Source UI live pendant le drag.
// invoke update_scene (save + snapshot :4321) seulement au create + pointerup.
import { writable, get } from "svelte/store";
import {
  tauri,
  MODULE_IDS,
  type Scene,
  type Widget,
  type CadreConfig,
  type MorphPoint,
  type ModuleId,
  type ModuleHud,
  type LigneObjectif,
} from "../tauri";
import { pointsClipPathCadre } from "../cadres/points";
import { obsHost, obsPort, obsPassword } from "./obs";
import { t } from "./langue";

export const sceneStore = writable<Scene>({
  widgets: [],
  canvasW: 1920,
  canvasH: 1080,
  bgMedia: "",
  bgKind: "image",
  bgFit: "remplir",
  bgZoom: 1,
  bgRot: 0,
  bgRotX: 0,
  bgRotY: 0,
  bgOffsetX: 0,
  bgOffsetY: 0,
  bgPaused: true,
  bgTime: 0,
});
export const loadedStore = writable(false);
export const selectedIdStore = writable<string | null>(null);

/// Coupe la capture input-viewer quand la scène chargée n'a plus de widget
/// input-viewer (économie : pas de poll clavier/manette 60Hz ni de hook
/// souris WH_MOUSE_LL sans rendu). Ne DÉMARRE jamais la capture — si la
/// scène a un widget, le bouton ON/OFF de sa carte reste le choix de
/// l'utilisateur. Non-fatal : set_actif(false) no-op côté Rust si déjà OFF.
async function syncInputViewerCapture(): Promise<void> {
  const aWidget = get(sceneStore).widgets.some((w) => w.type === "input-viewer");
  if (aWidget) return;
  try {
    await tauri.inputViewerSetActif(false);
  } catch {
    // Backend indisponible → silencieux (la capture sera recoupée au
    // prochain chargement/suppression).
  }
}

/// Charge la scène initiale depuis Rust (config.json).
export async function loadScene(): Promise<void> {
  try {
    const scene = await tauri.getScene();
    sceneStore.set(scene);
    // La nouvelle scène n'a peut-être pas de widget input-viewer → couper
    // la capture si elle tournait encore pour une scène précédente.
    await syncInputViewerCapture();
  } catch (e) {
    console.error("loadScene:", e);
  } finally {
    loadedStore.set(true);
  }
}

/// Ajoute un widget à la scène. Si la scène n'a JAMAIS eu de cadre configuré
/// (cadreWidget absent), applique le cadre par défaut MGStyle — un cadre
/// désactivé par l'utilisateur (actif:false) n'est PAS réactivé : son choix
/// prime sur le défaut. Un seul update → un seul commitScene.
function sceneAvecWidget(s: Scene, w: Widget): Scene {
  return {
    ...s,
    cadreWidget: s.cadreWidget ?? { ...CADRE_WIDGET_DEFAUT },
    widgets: [...s.widgets, w],
  };
}

/// Ajoute un widget par défaut (200×150, type media, z auto, id unique).
/// z calculé SANS les widgets caméra (plage réservée 5000+) → reste 0..N.
/// Commit immédiat → save config.json + snapshot WS. Retourne l'id créé.
export async function createWidget(): Promise<string> {
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
    mediaMiroirH: false,
    mediaMiroirV: false,
    mediaPaused: true,
    mediaTime: 0,
  };
  sceneStore.update((s) => sceneAvecWidget(s, w));
  await commitScene();
  return w.id;
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
    chatStyle: "compact",
  };
  sceneStore.update((s) => sceneAvecWidget(s, w));
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
  sceneStore.update((s) => sceneAvecWidget(s, w));
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
  sceneStore.update((s) => sceneAvecWidget(s, w));
  await commitScene();
}

/// Ajoute un widget objectif (280×90, type goal). Compteur live + cible
/// rendus dans diffusion.html via les frames WS "communaute"/"viewers" +
/// célébration interne au widget sur "goal-atteint" (emit viewers.rs).
/// Défaut : twitch/followers cible 10 — reconfiguré dans la carte d'édition
/// (plateforme, métrique, cible, suggestion de palier sur la valeur live).
/// Commit immédiat → save + snapshot WS.
export async function createGoalWidget(): Promise<void> {
  const current = get(sceneStore);
  const z = current.widgets.filter((w) => w.type !== "camera").reduce((m, w) => Math.max(m, w.z), -1) + 1;
  const w: Widget = {
    id: crypto.randomUUID(),
    type: "goal",
    x: 100,
    y: 100,
    largeur: 280,
    hauteur: 90,
    z,
    rotateX: 0,
    rotateY: 0,
    goalPlateforme: "twitch",
    goalMetrique: "followers",
    goalCible: 10,
  };
  sceneStore.update((s) => sceneAvecWidget(s, w));
  await commitScene();
}

/// Ajoute un widget musique (340×140, type musique). Mini-lecteur Winamp
/// rendu dans diffusion.html : LCD marquee (titre — demandeur) + temps +
/// équalizer spectre réel (audio ombre + AnalyserNode) + barre de
/// progression. musiqueMode "player" (défaut) = tout, "eq" = bandes seules.
/// Commit immédiat → save + snapshot WS.
export async function createMusiqueWidget(): Promise<void> {
  const current = get(sceneStore);
  const z = current.widgets.filter((w) => w.type !== "camera").reduce((m, w) => Math.max(m, w.z), -1) + 1;
  const w: Widget = {
    id: crypto.randomUUID(),
    type: "musique",
    x: 100,
    y: 100,
    largeur: 340,
    hauteur: 140,
    z,
    rotateX: 0,
    rotateY: 0,
    musiqueMode: "player",
  };
  sceneStore.update((s) => sceneAvecWidget(s, w));
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
  sceneStore.update((s) => sceneAvecWidget(s, w));
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
  // Découpe la caméra à la forme du cadre dès la création (retry interne
  // Rust si le capteur démarre ; non-fatal → auto-guérison au prochain
  // commit si le masque n'a pas pu être appliqué).
  await syncCameraMask();
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
  // forcer : un nouveau device peut changer la résolution native de la
  // source → le masque doit être recalculé même si la forme n'a pas bougé.
  await syncCameraMask(true);
}

/// Points du polygone du masque caméra (coords widget-local) — la forme
/// EXACTE du trou destination-out découpé par diffusion.html : mêmes
/// conditions (cadre actif + strokeWidth > 0 + pas de sansCadre), même
/// générateur (registre TS = port du vanilla JS de diffusion.html).
/// null = pas de forme → Rust retire les filtres, caméra rectangulaire.
function pointsMasqueCamera(scene: Scene): [number, number][] | null {
  const cam = scene.widgets.find((w) => w.type === "camera");
  if (!cam || cam.sansCadre === true) return null;
  const cadre = scene.cadreWidget;
  if (!cadre || !cadre.actif || (cadre.strokeWidth || 0) <= 0) return null;
  return pointsClipPathCadre(cadre.style, cam.largeur, cam.hauteur, cadre.strokeWidth || 4);
}

/// Clé du dernier masque synchronisé (JSON des points ou "sans-forme") :
/// un commit sans changement de forme ne re-synchronise rien.
let cleMasqueCamera = "";

/// Découpe SOS-Caméra + SOS-Caméra-Fond à la forme du cadre du widget
/// caméra (filtre OBS « Image mask/Blend », PNG généré par Rust en pixels
/// de la source). Cache par forme : le masque ne dépend NI de la position
/// (pixels de la source) NI des autres widgets — un move pur est un no-op.
/// `forcer` = bust du cache (changement de device : résolution native
/// potentiellement différente). La clé n'est mémorisée que si Rust
/// retourne true (false = capteur en démarrage → retente au prochain
/// commit, auto-guérison). Non-fatal : OBS offline → silencieux.
export async function syncCameraMask(forcer = false): Promise<void> {
  const scene = get(sceneStore);
  const cam = scene.widgets.find((w) => w.type === "camera");
  if (!cam) {
    // Plus de caméra : reset (les filtres restent sur les inputs cachés,
    // inoffensifs — la recréation du widget re-synchronisera).
    cleMasqueCamera = "";
    return;
  }
  const points = pointsMasqueCamera(scene);
  const cle = points ? JSON.stringify(points) : "sans-forme";
  if (!forcer && cle === cleMasqueCamera) return;
  try {
    // fit/zoom : les constantes de l'item caméra de commitScene (« remplir »,
    // pas de zoom/rot/offset en v1) — Rust mappe via le même pipeline.
    const applique = await tauri.cameraMaskSync(
      get(obsHost), parseInt(get(obsPort), 10), get(obsPassword),
      points, cam.largeur, cam.hauteur, "remplir", 1
    );
    if (applique) {
      cleMasqueCamera = cle;
    }
  } catch {
    // OBS offline → silencieux (auto-guérison au prochain sync).
  }
}

/// Hooks post-sync captures (ex. Just Chatting : renvoyer la scène DÉRIVÉE
/// après le sync normal pour re-cacher les trous/fond de la nouvelle scène).
/// Enregistrés via registerApresSyncCaptures — évite l'import inverse
/// (justChatting.ts importe déjà scene.ts). Un hook appelle le wrapper Tauri
/// directement, JAMAIS syncCapturesOBS (sinon re-enchaînement → boucle).
const apresSyncCapturesHooks: Array<() => Promise<void> | void> = [];
export function registerApresSyncCaptures(
  fn: () => Promise<void> | void
): void {
  apresSyncCapturesHooks.push(fn);
}

/// Exécute les hooks post-sync (séquentiel, non-fatal chacun).
async function runApresSyncCaptures(): Promise<void> {
  for (const fn of apresSyncCapturesHooks) {
    try {
      await fn();
    } catch {
      // Hook non-fatal (ex. sync dérivée Just Chatting).
    }
  }
}

/// Sync captures OBS de la scène chargée + masque caméra. Remplace les
/// appels directs tauri.sceneSyncCaptures des chargements de scène (boot,
/// openScene, preset, connexion OBS) : le masque doit suivre la scène
/// chargée (forme/taille du cadre caméra de CETTE scène). Non-fatal.
export async function syncCapturesOBS(): Promise<void> {
  try {
    await tauri.sceneSyncCaptures(
      get(obsHost), parseInt(get(obsPort), 10), get(obsPassword)
    );
  } catch {
    // OBS offline → sources caméra créées à la prochaine connexion
    // (ensure_camera au scene_sync_captures du connect — auto-guérison).
  }
  await syncCameraMask();
  await runApresSyncCaptures();
}

/// Change la mise en forme des messages d'un widget chat ("compact" |
/// "bulles"). Maj locale + commit.
export async function setChatStyle(id: string, style: string): Promise<void> {
  sceneStore.update((s) => ({
    ...s,
    widgets: s.widgets.map((w) =>
      w.id === id ? { ...w, chatStyle: style } : w
    ),
  }));
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

/// Maj locale de la taille de police chat (pendant le geste slider). PAS
/// d'invoke — le commit se fait au change/pointerup (pattern setMediaZoomLocal).
/// Clamp 8–48.
export function setChatTaillePoliceLocal(id: string, px: number): void {
  const p = Math.max(8, Math.min(48, Math.round(px)));
  sceneStore.update((s) => ({
    ...s,
    widgets: s.widgets.map((w) =>
      w.id === id ? { ...w, taillePolice: p } : w
    ),
  }));
}

/// Change la taille de police d'un widget chat. Maj locale + commit.
export async function setChatTaillePolice(id: string, px: number): Promise<void> {
  setChatTaillePoliceLocal(id, px);
  await commitScene();
}

/// Change la police Google Font + taille de base d'un widget speedrun.
/// police = id de police (voir POLICES_TITRE dans fonts.ts). taille = px (8-48).
/// Maj locale + commit. Toutes les tailles internes du splitter sont en em →
/// suit cette base (comme la taille de police du chat).
/// Maj locale police + taille speedrun (pendant le geste slider). PAS
/// d'invoke — commit au change. Clamp taille 8–48.
export function setSpeedrunPoliceLocal(id: string, police: string, taille: number): void {
  const t = Math.max(8, Math.min(48, Math.round(taille)));
  sceneStore.update((s) => ({
    ...s,
    widgets: s.widgets.map((w) =>
      w.id === id ? { ...w, srPolice: police, srTaillePolice: t } : w
    ),
  }));
}

export async function setSpeedrunPolice(
  id: string,
  police: string,
  taille: number
): Promise<void> {
  setSpeedrunPoliceLocal(id, police, taille);
  await commitScene();
}

/// Change la config des titres d'un widget (merge partiel + commit).
/// Jusqu'à 4 textes (titreHaut/Bas/Gauche/Droite, vide/undefined = aucun
/// titre à cette position) + style partagé. Passer un texte undefined
/// supprime le titre de cette position.
export async function setWidgetTitre(
  id: string,
  patch: Partial<Pick<Widget, "titreHaut" | "titreBas" | "titreGauche" | "titreDroite" | "titreCouleur" | "titreCouleurFin" | "titrePolice" | "titreTaille" | "titreGras" | "titreItalique" | "titreSouligne" | "titreEspacement" | "titreContourCouleur" | "titreContourEpaisseur">>
): Promise<void> {
  sceneStore.update((s) => ({
    ...s,
    widgets: s.widgets.map((w) => (w.id === id ? { ...w, ...patch } : w)),
  }));
  await commitScene();
}

/// Change la config des titres du fond de l'application (merge partiel +
/// commit). Jusqu'à 4 textes (bgTitreHaut/Bas/Gauche/Droite, vide/undefined =
/// aucun titre à cette position) + style partagé. Passer un texte undefined
/// supprime le titre de cette position.
export async function setBgTitre(
  patch: Partial<Pick<Scene, "bgTitreHaut" | "bgTitreBas" | "bgTitreGauche" | "bgTitreDroite" | "bgTitreCouleur" | "bgTitreCouleurFin" | "bgTitrePolice" | "bgTitreTaille" | "bgTitreGras" | "bgTitreItalique" | "bgTitreSouligne" | "bgTitreEspacement" | "bgTitreContourCouleur" | "bgTitreContourEpaisseur">>
): Promise<void> {
  sceneStore.update((s) => ({ ...s, ...patch }));
  await commitScene();
}

/// Maj locale de la config input viewer (pendant le geste — ex. color picker
/// oninput continu). PAS d'invoke — commit au change.
export function setInputViewerConfigLocal(
  id: string,
  patch: Partial<Pick<Widget, "inputMode" | "inputLayout" | "inputSkin" | "inputCouleur" | "inputMapping">>
): void {
  sceneStore.update((s) => ({
    ...s,
    widgets: s.widgets.map((w) => (w.id === id ? { ...w, ...patch } : w)),
  }));
}

/// Change la config d'un widget input viewer (merge partiel + commit).
export async function setInputViewerConfig(
  id: string,
  patch: Partial<Pick<Widget, "inputMode" | "inputLayout" | "inputSkin" | "inputCouleur" | "inputMapping">>
): Promise<void> {
  setInputViewerConfigLocal(id, patch);
  await commitScene();
}

/// Change la config d'un widget speedrun (chemins ASL/LSS + settings ASL +
/// disposition). Merge partiel + commit. Les chemins permettent le
/// rechargement auto au montage du widget ; les settings sont aussi globaux
/// (moteur unique).
export async function setSpeedrunConfig(
  id: string,
  patch: Partial<Pick<Widget, "speedrunCheminAsl" | "speedrunCheminLss" | "speedrunSettings" | "srLayout">>
): Promise<void> {
  sceneStore.update((s) => ({
    ...s,
    widgets: s.widgets.map((w) => (w.id === id ? { ...w, ...patch } : w)),
  }));
  await commitScene();
}

/// Change la config d'un widget goal (plateforme, métrique, cible, goal lié).
/// Merge partiel + commit → snapshot WS → la diffusion met à jour le rendu.
/// Passer goalCible: undefined dans le patch ne supprime pas la clé (merge
/// {...w, ...patch}) — utiliser la forme { goalCible: <nombre> }.
/// goalId: id du Creator Goal Twitch à lier (mode lié) ; goalId: undefined
/// DÉLIE le widget (retour mode manuel — la clé est omise à la sérialisation).
/// Changer de plateforme hors twitch doit accompagner goalId: undefined
/// (les Creator Goals n'existent que sur Twitch).
export async function setGoalConfig(
  id: string,
  patch: Partial<
    Pick<Widget, "goalPlateforme" | "goalMetrique" | "goalCible" | "goalId">
  >
): Promise<void> {
  sceneStore.update((s) => ({
    ...s,
    widgets: s.widgets.map((w) => (w.id === id ? { ...w, ...patch } : w)),
  }));
  await commitScene();
}

/// Change le mode d'affichage d'un widget musique ("player" | "eq").
/// Merge partiel + commit → snapshot WS → la diffusion re-render le widget.
export async function setMusiqueConfig(
  id: string,
  musiqueMode: "player" | "eq"
): Promise<void> {
  sceneStore.update((s) => ({
    ...s,
    widgets: s.widgets.map((w) => (w.id === id ? { ...w, musiqueMode } : w)),
  }));
  await commitScene();
}

// ===== Modules HUD (horloge / musique en cours / objectifs du stream) =====
// Couche d'affichage SÉPARÉE des widgets : `scene.modules.<id>`, un
// emplacement par module. Transportée par le snapshot WS comme le reste de
// la scène (commitScene) — zéro state Rust dédié.

/// Défauts à la première activation d'un module — positions reprises de la
/// maquette de référence (1920×1080 : horloge bas-centre, musique bas-droite,
/// objectifs bas-gauche), mises à l'échelle du canvas courant.
export function moduleDefaut(id: ModuleId, canvasW: number, canvasH: number): ModuleHud {
  const sx = canvasW / 1920;
  const sy = canvasH / 1080;
  const base = { actif: true, police: undefined, cadre: false, opacite: 0.45 };
  if (id === "horloge") {
    return { ...base, x: 760 * sx, y: 940 * sy, largeur: 400 * sx, hauteur: 60 * sy,
      afficherDate: true, secondes: false, format12h: false };
  }
  if (id === "musique") {
    return { ...base, x: 1320 * sx, y: 830 * sy, largeur: 540 * sx, hauteur: 90 * sy,
      afficherTemps: true };
  }
  return { ...base, x: 70 * sx, y: 720 * sy, largeur: 300 * sx, hauteur: 170 * sy,
    titre: "", lignes: [] };
}

/// Lit un module de la scène courante (undefined = jamais configuré).
export function moduleCourant(id: ModuleId): ModuleHud | undefined {
  return get(sceneStore).modules?.[id];
}

/// Merge partiel d'un module + commit. Le module est créé avec ses défauts
/// s'il n'existait pas (patch appliqué par-dessus). Utilisé par le drag/
/// resize (x/y/largeur/hauteur au pointerup) et la modale (options).
export async function setModule(id: ModuleId, patch: Partial<ModuleHud>): Promise<void> {
  sceneStore.update((s) => {
    const existant = s.modules?.[id] ?? moduleDefaut(id, s.canvasW ?? 1920, s.canvasH ?? 1080);
    return { ...s, modules: { ...(s.modules ?? {}), [id]: { ...existant, ...patch } } };
  });
  await commitScene();
}

/// Active/désactive un module. L'activation d'un module jamais configuré
/// le crée aux défauts ; la désactivation conserve position et options.
export async function setModuleActif(id: ModuleId, actif: boolean): Promise<void> {
  await setModule(id, { actif });
}

/// Objectifs : ajoute une ligne (métrique mesurée par défaut twitch/followers).
export async function ajouterLigneObjectif(ligne?: Partial<LigneObjectif>): Promise<void> {
  const lignes = moduleCourant("objectifs")?.lignes ?? [];
  const nouvelle: LigneObjectif = {
    id: crypto.randomUUID(),
    plateforme: "twitch",
    metrique: "followers",
    ...ligne,
  };
  await setModule("objectifs", { lignes: [...lignes, nouvelle] });
}

/// Objectifs : merge partiel d'une ligne. Passer `goalId: undefined` délie
/// (la clé est omise à la sérialisation) ; `cible: undefined` retire la cible.
export async function setLigneObjectif(ligneId: string, patch: Partial<LigneObjectif>): Promise<void> {
  const lignes = moduleCourant("objectifs")?.lignes ?? [];
  await setModule("objectifs", {
    lignes: lignes.map((l) => (l.id === ligneId ? { ...l, ...patch } : l)),
  });
}

/// Objectifs : retire une ligne.
export async function retirerLigneObjectif(ligneId: string): Promise<void> {
  const lignes = moduleCourant("objectifs")?.lignes ?? [];
  await setModule("objectifs", { lignes: lignes.filter((l) => l.id !== ligneId) });
}

/// Objectifs : déplace une ligne d'un cran (delta -1 = monter, +1 = descendre).
export async function deplacerLigneObjectif(ligneId: string, delta: -1 | 1): Promise<void> {
  const lignes = [...(moduleCourant("objectifs")?.lignes ?? [])];
  const i = lignes.findIndex((l) => l.id === ligneId);
  const j = i + delta;
  if (i < 0 || j < 0 || j >= lignes.length) return;
  [lignes[i], lignes[j]] = [lignes[j], lignes[i]];
  await setModule("objectifs", { lignes });
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

/// Bascule le miroir du média (flip H/V du contenu, scale ±1 combiné au
/// zoom dans le rendu). Changement discret → maj locale + commit immédiat.
export async function toggleMediaMiroir(id: string, sens: "h" | "v"): Promise<void> {
  sceneStore.update((s) => ({
    ...s,
    widgets: s.widgets.map((w) =>
      w.id === id
        ? sens === "h"
          ? { ...w, mediaMiroirH: !(w.mediaMiroirH ?? false) }
          : { ...w, mediaMiroirV: !(w.mediaMiroirV ?? false) }
        : w
    ),
  }));
  await commitScene();
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

/// Reset transformation média : zoom 1, rotation 0, offsets 0. Maj locale +
/// commit. NE touche PAS les effets (onglet Effets, cf. resetMediaEffets) —
/// l'ancien reset tout-en-un effaçait les effets depuis l'onglet
/// Transformation sans prévenir.
export async function resetMediaTransformation(id: string): Promise<void> {
  sceneStore.update((s) => ({
    ...s,
    widgets: s.widgets.map((w) =>
      w.id === id ? { ...w, mediaZoom: 1, mediaRot: 0, mediaOffsetX: 0, mediaOffsetY: 0 } : w
    ),
  }));
  await commitScene();
}

/// Reset effets média : luminosité/contraste/teinte/flou/pixel 0 + miroir
/// désactivé (contenu de l'onglet Effets — morphs exclus, bouton dédié).
/// Maj locale + commit.
export async function resetMediaEffets(id: string): Promise<void> {
  sceneStore.update((s) => ({
    ...s,
    widgets: s.widgets.map((w) =>
      w.id === id ? { ...w, mediaLum: 0, mediaContraste: 0, mediaTeinte: 0, mediaFlou: 0, mediaPixel: 0, mediaMiroirH: false, mediaMiroirV: false } : w
    ),
  }));
  await commitScene();
}

// Chaîne de commits : UNE SEULE update_scene en vol à la fois (voir
// commitScene ci-dessous).
let commitEnVol: Promise<void> = Promise.resolve();

/// Pousse la scène vers Rust → save config.json + snapshot WS.
/// Appelé au create et au pointerup (fin du drag).
/// SÉRIALISÉ : une seule update_scene en vol à la fois, le store lu DANS la
/// tâche (au moment de l'envoi) — deux actions rapides (import résolu +
/// duplication, double-clic Dupliquer) ne peuvent pas s'écraser avec une
/// scène obsolète : le dernier état part toujours en dernier.
/// Après updateScene : sync OBS des sources trou (one-shot reconnect) si ≥1
/// widget a obsSource (hors chaîne — ne retarde pas les commits suivants).
/// Erreur OBS → log discret, pas de crash.
export async function commitScene(): Promise<void> {
  // Guard : ne pas commiter tant que la scène n'est pas chargée depuis Rust.
  // Sinon on risque d'écraser la scène RAM (vraie scène chargée par boot_scenes)
  // avec la scène vide initiale du sceneStore → perte de données sur disque.
  if (!get(loadedStore)) {
    return;
  }
  const envoi = commitEnVol.then(() => tauri.updateScene(get(sceneStore)));
  // La chaîne ne casse jamais : une erreur est isolée au seul appel concerné.
  commitEnVol = envoi.catch(() => {});
  try {
    await envoi;
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
  // Fond capture : source plein canvas (sync_trous travaille en alignment
  // centre : cx = x + w/2 → 0 + canvasW/2 → couvre exactement le canvas).
  if (scene.bgObsSource) {
    items.push({
      sourceName: scene.bgObsSource,
      x: 0,
      y: 0,
      w: scene.canvasW ?? 1920,
      h: scene.canvasH ?? 1080,
      fit: "etirer",
      zoom: 1,
      rot: 0,
      offsetX: 0,
      offsetY: 0,
    });
  }
  if (items.length > 0) {
    try {
      await tauri.obsSyncTrous(
        get(obsHost), parseInt(get(obsPort), 10), get(obsPassword), items
      );
    } catch {
      // OBS offline → message discret, pas de crash. La scène reste OK.
    }
  }
  // Masque caméra : découpe SOS-Caméra/SOS-Caméra-Fond à la forme du cadre
  // (cache-skip interne — un move pur ne re-synchronise rien, le masque est
  // en pixels de la source, indépendant de la position).
  await syncCameraMask();
  // Just Chatting actif : obsSyncTrous vient de recaler SOS-Caméra sur le
  // rect réel du widget → le hook renvoie la scène dérivée + rect JC.
  await runApresSyncCaptures();
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
  // Le widget supprimé était peut-être le dernier input-viewer → couper la
  // capture si elle tournait (plus de rendu possible).
  await syncInputViewerCapture();
}

/// Duplique un widget : copie tous les champs sauf id (nouvel uuid), z
/// (au-dessus des widgets non-caméra) et obsSource (une source OBS = un seul
/// emplacement — la copie d'un trou lié reste un trou NON lié, l'utilisateur
/// relie une autre source via « Lier une source OBS »).
/// Position initiale : miroir sur l'axe vertical du canvas (marge gauche →
/// marge droite égale). Si l'original est centré (miroir == même place),
/// décalage +24px pour distinguer la copie.
/// La copie devient le widget sélectionné. Commit immédiat → save + snapshot.
/// Retourne l'id créé, null si widget introuvable ou caméra (singleton figé).
export async function dupliquerWidget(id: string): Promise<string | null> {
  const current = get(sceneStore);
  const src = current.widgets.find((w) => w.id === id);
  if (!src || src.type === "camera") return null;
  const canvasW = current.canvasW ?? 1920;
  const canvasH = current.canvasH ?? 1080;
  const z =
    current.widgets
      .filter((w) => w.type !== "camera")
      .reduce((m, w) => Math.max(m, w.z), -1) + 1;
  let x = canvasW - src.x - src.largeur;
  if (Math.abs(x - src.x) < 1) x += 24;
  const copie: Widget = clampWidget(
    { ...src, id: crypto.randomUUID(), x, y: src.y, z, obsSource: undefined },
    canvasW,
    canvasH
  );
  sceneStore.update((s) => sceneAvecWidget(s, copie));
  selectedIdStore.set(copie.id);
  await commitScene();
  return copie.id;
}

/// Place un widget tout au premier plan ("avant") ou tout en arrière-plan
/// ("arriere") de la pile. La caméra (z=5000, plage réservée) est EXCLUE du
/// tri : les widgets normaux restent dans la bande 0..N-1, jamais au-dessus
/// d'elle, et elle n'a pas de boutons -/+ (Widget.svelte).
/// La bande non-caméra est renumérotée à chaque appel → z contigus, les
/// trous laissés par les deletes sont comblés au passage (z négatif
/// impossible : un widget < 0 passerait SOUS le fond .bg-layer z:0).
/// Maj locale + commit (save + snapshot :4321 — le z est « style seulement »
/// côté diffusion, aucun rebuild). No-op sur la caméra.
export async function mettreWidgetPlan(
  id: string,
  sens: "avant" | "arriere"
): Promise<void> {
  sceneStore.update((s) => {
    const tri = s.widgets
      .filter((w) => w.type !== "camera")
      .sort((a, b) => a.z - b.z);
    const idx = tri.findIndex((w) => w.id === id);
    if (idx === -1) return s;
    const [w] = tri.splice(idx, 1);
    if (sens === "avant") {
      tri.push(w);
    } else {
      tri.unshift(w);
    }
    const zParId = new Map(tri.map((x, i) => [x.id, i]));
    return {
      ...s,
      widgets: s.widgets.map((x) => {
        const z = zParId.get(x.id);
        return z !== undefined && z !== x.z ? { ...x, z } : x;
      }),
    };
  });
  await commitScene();
}

/// Déduit le kind ("image"|"video") d'un chemin relatif média.
const VIDEO_EXT = ["mp4", "webm"];
function kindFromMedia(rel: string): string {
  const ext = rel.split(".").pop()?.toLowerCase() ?? "";
  return VIDEO_EXT.includes(ext) ? "video" : "image";
}

/// Importe un média (image OU vidéo) pour le widget sélectionné : met à jour
/// le store local (chemin relatif retourné par Rust + kind déduit de
/// l'extension) puis COMMIT — l'import ne mute plus la scène côté Rust (un
/// seul écrivain). Affiche une alerte en cas de refus (taille/format).
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
      // Commit immédiat : l'import ne mute plus la scène côté Rust (un seul
      // écrivain — le store). Sans ça, le média ne serait persisté qu'au
      // prochain commit d'une autre action (perte si redémarrage avant).
      await commitScene();
    }
  } catch (e) {
    alert(get(t)("commun.importRefuse", { erreur: String(e) }));
  }
}

// ===== Carrousel d'images — mediaSlides (widget média) =====
// Liste non vide = ACTIF et prioritaire sur `media` (le média unique reste
// stocké, il redevient visible si la liste est vidée). Images uniquement.
// Le timer tourne côté diffusion.html — AUCUN commit par slide, seules les
// mutations de liste/intervalle commitent (pattern un seul écrivain).

/// Importe plusieurs images (dialog multi, filtre Images) et les AJOUTE à la
/// fin de la liste de slides du widget sélectionné. Erreurs par fichier
/// concaténées en alerte (comportement bibliothèque). Commit si ≥ 1 slide.
export async function ajouterSlidesMedia(): Promise<void> {
  const id = get(selectedIdStore);
  if (!id) return;
  try {
    const res = await tauri.importMediasMulti();
    if (res.ok.length > 0) {
      const slides = res.ok.map(([rel, _kind, nom]) => ({ rel, nom }));
      sceneStore.update((s) => ({
        ...s,
        widgets: s.widgets.map((w) =>
          w.id === id ? { ...w, mediaSlides: [...(w.mediaSlides ?? []), ...slides] } : w
        ),
      }));
      await commitScene();
    }
    if (res.erreurs.length > 0) {
      alert(get(t)("commun.importRefuseListe", { erreurs: res.erreurs.join("\n") }));
    }
  } catch (e) {
    alert(get(t)("commun.importRefuse", { erreur: String(e) }));
  }
}

/// Retire la slide d'index i de la liste du widget sélectionné. Liste vidée
/// → mediaSlides repasse undefined (carrousel off, `media` redevient visible).
/// Ne supprime PAS le fichier dans medias/ (même règle que clearWidgetMedia).
export async function retirerSlideMedia(i: number): Promise<void> {
  const id = get(selectedIdStore);
  if (!id) return;
  sceneStore.update((s) => ({
    ...s,
    widgets: s.widgets.map((w) => {
      if (w.id !== id || !w.mediaSlides) return w;
      const slides = w.mediaSlides.filter((_, j) => j !== i);
      return { ...w, mediaSlides: slides.length ? slides : undefined };
    }),
  }));
  await commitScene();
}

/// Déplace la slide d'index i de dir (-1 = monter, +1 = descendre) dans la
/// liste du widget sélectionné. Bornes ignorées silencieusement. Commit.
export async function deplacerSlideMedia(i: number, dir: -1 | 1): Promise<void> {
  const id = get(selectedIdStore);
  if (!id) return;
  sceneStore.update((s) => ({
    ...s,
    widgets: s.widgets.map((w) => {
      if (w.id !== id || !w.mediaSlides) return w;
      const j = i + dir;
      if (j < 0 || j >= w.mediaSlides.length) return w;
      const slides = [...w.mediaSlides];
      [slides[i], slides[j]] = [slides[j], slides[i]];
      return { ...w, mediaSlides: slides };
    }),
  }));
  await commitScene();
}

/// Intervalle du carrousel du widget sélectionné (secondes, clamp 1–3600).
/// Maj locale + commit immédiat (changement discret, comme setMediaFit).
export async function setSlidesSecMedia(sec: number): Promise<void> {
  const id = get(selectedIdStore);
  if (!id) return;
  const v = Math.max(1, Math.min(3600, sec));
  sceneStore.update((s) => ({
    ...s,
    widgets: s.widgets.map((w) =>
      w.id === id ? { ...w, mediaSlidesSec: v } : w
    ),
  }));
  await commitScene();
}

/// Vide la liste de slides du widget sélectionné (carrousel off → `media`
/// redevient visible). Ne supprime PAS les fichiers dans medias/. Commit.
export async function viderSlidesMedia(): Promise<void> {
  const id = get(selectedIdStore);
  if (!id) return;
  sceneStore.update((s) => ({
    ...s,
    widgets: s.widgets.map((w) =>
      w.id === id ? { ...w, mediaSlides: undefined, mediaSlidesSec: undefined } : w
    ),
  }));
  await commitScene();
}

// ===== Bibliothèque médias (import par chemins — drop Tauri) =====

/// Entrée de la bibliothèque médias : fichier copié dans medias/ via
/// import_media_from_paths, pas encore transformé en widget.
export interface MediaBibliotheque {
  /// Chemin relatif ("medias/<uuid>.<ext>").
  rel: string;
  /// Kind retourné par Rust : "image" | "video".
  kind: string;
  /// Nom original du fichier (avec extension, sans chemin).
  nom: string;
}

/// Bibliothèque des médias importés par chemins disque (drop Tauri).
/// Volatile (pas persistée) : les fichiers restent dans medias/ sur disque.
export const bibliothequeMedias = writable<MediaBibliotheque[]>([]);

/// Importe des médias depuis des chemins disque (drop Tauri — pas de dialog)
/// puis pose chacun en widget média immédiatement (auto-pose, sansCadre).
/// Mêmes règles qu'importMedia (validation + copie/remux vers medias/).
/// Un échec sur un fichier n'annule pas les autres : les erreurs sont
/// affichées concaténées. Les entrées RESTENT dans bibliothequeMedias après
/// la pose : le bouton « Transformer en widget ! » (Toolbar, widget sansCadre
/// sélectionné) convertit ensuite le widget déjà posé (sansCadre → cadre
/// normal) et consomme la ligne à ce moment-là.
export async function ajouterALaBibliotheque(paths: string[]): Promise<void> {
  if (paths.length === 0) return;
  try {
    const res = await tauri.importMediaFromPaths(paths);
    if (res.ok.length > 0) {
      const entries: MediaBibliotheque[] = res.ok.map(([rel, kind, nom]) => ({
        rel,
        kind,
        nom,
      }));
      bibliothequeMedias.update((l) => [...l, ...entries]);
      // Auto-pose séquentielle : chaque média devient un widget média
      // (sansCadre) sur le canvas + :4321 tout de suite. La ligne de
      // bibliothèque est conservée (transformerEnWidget la consommera).
      for (const [rel] of res.ok) {
        await poserMediaDepuisBibliotheque(rel);
      }
    }
    if (res.erreurs.length > 0) {
      alert(get(t)("commun.importRefuseListe", { erreurs: res.erreurs.join("\n") }));
    }
  } catch (e) {
    alert(get(t)("commun.importRefuse", { erreur: String(e) }));
  }
}

/// Pose une entrée de la bibliothèque sur le canvas : crée le widget via
/// createWidget(), remplit ses champs média (media + kind + mediaNom) et
/// sansCadre:true (opt-out du cadre SVG de scène — les widgets droppés
/// s'affichent bruts), puis commit. NE retire PAS l'entrée de la
/// bibliothèque : elle reste listée dans le DropBar (transformerEnWidget la
/// consommera au clic du bouton Toolbar).
/// Si createWidget échoue → stop, pas de widget fantôme.
export async function poserMediaDepuisBibliotheque(rel: string): Promise<void> {
  const entry = get(bibliothequeMedias).find((m) => m.rel === rel);
  if (!entry) return;
  const id = await createWidget();
  sceneStore.update((s) => ({
    ...s,
    widgets: s.widgets.map((w) =>
      w.id === id ? { ...w, media: entry.rel, kind: entry.kind, mediaNom: entry.nom, sansCadre: true } : w
    ),
  }));
  await commitScene();
}

/// Convertit un média en widget classique (avec cadre de scène) — bouton
/// « Transformer en widget ! » de la Toolbar (widget sélectionné sansCadre).
/// Ne crée PAS de doublon : si un widget référence déjà ce média (auto-pose
/// au drop), on repasse simplement sansCadre à false sur CE widget — filet
/// dashboard + cadre SVG :4321 comme un média importé à la main. Sinon (pas
/// encore de widget pour ce rel) on pose d'abord via
/// poserMediaDepuisBibliotheque puis on enlève le sansCadre. L'entrée est
/// retirée de la bibliothèque si présente (no-op sinon — le store est
/// volatile, un widget sansCadre rechargé fonctionne quand même) puis commit.
export async function transformerEnWidget(rel: string): Promise<void> {
  const dejaPose = get(sceneStore).widgets.some((w) => w.media === rel);
  if (!dejaPose) {
    await poserMediaDepuisBibliotheque(rel);
  }
  sceneStore.update((s) => ({
    ...s,
    widgets: s.widgets.map((w) =>
      w.media === rel ? { ...w, sansCadre: false } : w
    ),
  }));
  bibliothequeMedias.update((l) => l.filter((m) => m.rel !== rel));
  await commitScene();
}

/// Repasse un widget média en « flottant » (sansCadre: true — opt-out du
/// cadre de scène, affichage brut comme au drop). Symétrique de
/// transformerEnWidget, sans toucher la bibliothèque : l'entrée drop a déjà
/// été consommée au passage en widget, et un média posé hors bibliothèque
/// n'a pas de ligne à recréer. Puis commit.
export async function transformerEnFlottant(id: string): Promise<void> {
  await setWidgetSansCadre(id, true);
}

/// Active/retire le cadre de scène sur UN widget (flag sansCadre — opt-out
/// par instance, valable pour tous les types : média, chat, caméra, …).
/// Réactivation (v=false) : si le widget porte un média encore listé dans la
/// bibliothèque (drop), l'entrée est consommée — sémantique identique à
/// transformerEnWidget. Commit immédiat.
export async function setWidgetSansCadre(id: string, v: boolean): Promise<void> {
  const w = get(sceneStore).widgets.find((x) => x.id === id);
  sceneStore.update((s) => ({
    ...s,
    widgets: s.widgets.map((x) =>
      x.id === id ? { ...x, sansCadre: v } : x
    ),
  }));
  if (!v && w?.media) {
    bibliothequeMedias.update((l) => l.filter((m) => m.rel !== w.media));
  }
  await commitScene();
}

/// Pose la jacquette du jeu Twitch sur la scène courante. Ordre fixe :
/// 1) twitch_live_poser_jacquette télécharge le JPEG dans
///    medias/jacquette-<gameId>.jpg (écrasé) — échec ⇒ scène intacte ;
/// 2) upsert : un widget image dont `media` finit par ce nom de fichier est
///    mis à jour (media + kind + mediaNom seulement), sinon créé ~190×253
///    en haut à gauche (z hors plage caméra, mêmes défauts que createWidget) ;
/// 3) un seul commitScene. Pas de game_id persisté sur le widget — le nom
///    de fichier sert de clé de déduplication (un second clic n'empile pas).
export async function poserJacquette(
  boxArtUrl: string,
  gameId: string,
  nom: string
): Promise<void> {
  const rel = await tauri.twitchLivePoserJacquette(boxArtUrl, gameId);
  const nomFichier = `jacquette-${gameId}.jpg`;
  const estJacquette = (w: Widget): boolean =>
    w.type === "media" && (w.media ?? "").endsWith(nomFichier);
  const current = get(sceneStore);
  if (current.widgets.some(estJacquette)) {
    sceneStore.update((s) => ({
      ...s,
      widgets: s.widgets.map((w) =>
        estJacquette(w) ? { ...w, media: rel, kind: "image", mediaNom: nom } : w
      ),
    }));
  } else {
    const z = current.widgets
      .filter((w) => w.type !== "camera")
      .reduce((m, w) => Math.max(m, w.z), -1) + 1;
    const w: Widget = {
      id: crypto.randomUUID(),
      type: "media",
      x: 100,
      y: 100,
      largeur: 190,
      hauteur: 253,
      z,
      rotateX: 0,
      rotateY: 0,
      mediaFit: "ajuster",
      mediaZoom: 1,
      mediaRot: 0,
      mediaOffsetX: 0,
      mediaOffsetY: 0,
      mediaMiroirH: false,
      mediaMiroirV: false,
      mediaPaused: true,
      mediaTime: 0,
      media: rel,
      kind: "image",
      mediaNom: nom,
    };
    sceneStore.update((s) => sceneAvecWidget(s, w));
  }
  await commitScene();
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

/// Importe un média (image OU vidéo) comme fond de scène : met à jour le
/// store local (bgMedia + bgKind déduit de l'extension) puis COMMIT (un seul
/// écrivain, comme importMedia). Affiche une alerte en cas de refus
/// (taille/format). Mêmes limites que importMedia (helper commun Rust).
export async function importFond(): Promise<void> {
  try {
    const rel = await tauri.importFond();
    if (rel) {
      const kind = kindFromMedia(rel);
      sceneStore.update((s) => ({ ...s, bgMedia: rel, bgKind: kind }));
      // Commit immédiat (même règle que importMedia — un seul écrivain).
      await commitScene();
    }
  } catch (e) {
    alert(get(t)("commun.importRefuse", { erreur: String(e) }));
  }
}

// ===== Carrousel d'images — bgSlides (fond de scène) =====
// Liste non vide = ACTIF et prioritaire sur bgMedia (reste stocké).
// bgObsSource reste prioritaire sur tout (règle figée). Timer côté
// diffusion.html — AUCUN commit par slide.

/// Importe plusieurs images (dialog multi, filtre Images) et les AJOUTE à la
/// fin de la liste de slides du fond. Erreurs concaténées en alerte. Commit
/// si ≥ 1 slide.
export async function ajouterSlidesFond(): Promise<void> {
  try {
    const res = await tauri.importMediasMulti();
    if (res.ok.length > 0) {
      const slides = res.ok.map(([rel, _kind, nom]) => ({ rel, nom }));
      sceneStore.update((s) => ({ ...s, bgSlides: [...(s.bgSlides ?? []), ...slides] }));
      await commitScene();
    }
    if (res.erreurs.length > 0) {
      alert(get(t)("commun.importRefuseListe", { erreurs: res.erreurs.join("\n") }));
    }
  } catch (e) {
    alert(get(t)("commun.importRefuse", { erreur: String(e) }));
  }
}

/// Retire la slide d'index i de la liste du fond. Liste vidée → bgSlides
/// repasse undefined (bgMedia redevient visible). Commit.
export async function retirerSlideFond(i: number): Promise<void> {
  sceneStore.update((s) => {
    const slides = (s.bgSlides ?? []).filter((_, j) => j !== i);
    return { ...s, bgSlides: slides.length ? slides : undefined };
  });
  await commitScene();
}

/// Déplace la slide d'index i de dir (-1 = monter, +1 = descendre) dans la
/// liste du fond. Bornes ignorées silencieusement. Commit.
export async function deplacerSlideFond(i: number, dir: -1 | 1): Promise<void> {
  sceneStore.update((s) => {
    const cur = s.bgSlides;
    if (!cur) return s;
    const j = i + dir;
    if (j < 0 || j >= cur.length) return s;
    const slides = [...cur];
    [slides[i], slides[j]] = [slides[j], slides[i]];
    return { ...s, bgSlides: slides };
  });
  await commitScene();
}

/// Intervalle du carrousel de fond (secondes, clamp 1–3600). Maj locale +
/// commit immédiat.
export async function setSlidesSecFond(sec: number): Promise<void> {
  const v = Math.max(1, Math.min(3600, sec));
  sceneStore.update((s) => ({ ...s, bgSlidesSec: v }));
  await commitScene();
}

/// Vide la liste de slides du fond (carrousel off → bgMedia redevient
/// visible). Ne supprime PAS les fichiers dans medias/. Commit.
export async function viderSlidesFond(): Promise<void> {
  sceneStore.update((s) => ({ ...s, bgSlides: undefined, bgSlidesSec: undefined }));
  await commitScene();
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

/// Maj locale de l'inclinaison 3D du fond (pendant le drag boule gizmo).
/// PAS d'invoke — commit au pointerup. Clamp -90 … 90 (tilt seul, pas de
/// miroir : les flips du fond sont les toggles bgMiroirH/V). Miroir de
/// rotateWidgetLocal côté widgets.
export function rotateFondLocal(rx: number, ry: number): void {
  const cx = Math.max(-90, Math.min(90, rx));
  const cy = Math.max(-90, Math.min(90, ry));
  sceneStore.update((s) => ({ ...s, bgRotX: cx, bgRotY: cy }));
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

/// Bascule le miroir du fond (flip H/V du média de fond). Changement
/// discret → maj locale + commit immédiat.
export async function toggleBgMiroir(sens: "h" | "v"): Promise<void> {
  sceneStore.update((s) =>
    sens === "h" ? { ...s, bgMiroirH: !(s.bgMiroirH ?? false) } : { ...s, bgMiroirV: !(s.bgMiroirV ?? false) }
  );
  await commitScene();
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

/// Reset transformation fond : zoom 1, rotation 0, offsets 0. Maj locale +
/// commit. NE touche PAS les effets (cf. resetFondEffets) — même découpage
/// que les widgets média.
export async function resetFondTransformation(): Promise<void> {
  sceneStore.update((s) => ({ ...s, bgZoom: 1, bgRot: 0, bgRotX: 0, bgRotY: 0, bgOffsetX: 0, bgOffsetY: 0 }));
  await commitScene();
}

/// Reset effets fond : luminosité/contraste/teinte/flou/pixel 0 + miroir
/// désactivé (contenu de l'onglet Effets). Maj locale + commit.
export async function resetFondEffets(): Promise<void> {
  sceneStore.update((s) => ({ ...s, bgLum: 0, bgContraste: 0, bgTeinte: 0, bgFlou: 0, bgPixel: 0, bgMiroirH: false, bgMiroirV: false }));
  await commitScene();
}

/// Supprime le fond : retire la référence bgMedia (garde bgKind/bgFit/bgZoom/
/// bgRot). Maj locale + commit. Ne supprime PAS le fichier dans medias/.
export async function clearFond(): Promise<void> {
  sceneStore.update((s) => ({ ...s, bgMedia: "" }));
  await commitScene();
}

// ===== Fond de scène = capture OBS (app/jeu plein canvas) =====

/// Lie une capture (fenêtre/jeu) comme FOND de la scène : crée/réutilise la
/// source OBS "SOS-Fond-<sceneId8>" PLEIN CANVAS via le pipeline trou
/// (obsCreateTrouFromPc = create_trou_from_pc : CreateInput/reuse + transform
/// + reorder tout en bas de pile). bgObsSource est PRIORITAIRE sur bgMedia —
/// le média stocké n'est pas peint tant que la capture est active. Store +
/// commit (resync transform + snapshot).
export async function lierFondCapture(
  kind: "camera" | "window" | "game",
  target: string | null
): Promise<string> {
  const s = get(sceneStore);
  // Id de la scène courante → nom de source PAR SCÈNE (2 scènes = 2 fenêtres
  // capturées différentes ; la sync active la bonne, cache les autres).
  const entry = await tauri.sceneCourante();
  const sourceName = "SOS-Fond-" + entry.id.slice(0, 8);
  const name = await tauri.obsCreateTrouFromPc(
    get(obsHost), parseInt(get(obsPort), 10), get(obsPassword),
    sourceName, kind, target, 0, 0, s.canvasW ?? 1920, s.canvasH ?? 1080
  );
  sceneStore.update((st) => ({ ...st, bgObsSource: name }));
  await commitScene();
  return name;
}

/// Retire la capture du fond : supprime la source OBS liée (RemoveInput —
/// pattern widget trou, pas de conflit d'accès exclusif pour fenêtre/jeu)
/// puis clear bgObsSource + commit. Le fond média stocké (s'il existe)
/// redevient actif.
export async function retirerFondCapture(): Promise<void> {
  const src = get(sceneStore).bgObsSource;
  if (src) {
    try {
      await tauri.obsDeleteTrouSource(
        get(obsHost), parseInt(get(obsPort), 10), get(obsPassword), src
      );
    } catch {
      // OBS offline → on clear quand même côté scène (pattern setWidgetTrou).
    }
  }
  sceneStore.update((s) => ({ ...s, bgObsSource: undefined }));
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

/// Maj locale de la position vidéo (pendant le drag du slider seek). PAS
/// d'invoke — la vignette dashboard suit en live, le commit (snapshot :4321)
/// se fait au release (onchange). Clamp ≥ 0.
export function setWidgetMediaTimeLocal(id: string, time: number): void {
  const t = Math.max(0, time);
  sceneStore.update((s) => ({
    ...s,
    widgets: s.widgets.map((w) =>
      w.id === id ? { ...w, mediaTime: t } : w
    ),
  }));
}

/// Change la position (seek) d'un widget vidéo → commit (snapshot :4321).
/// Dashboard : applique mediaTime une fois pour la vignette (frame figée).
export async function setWidgetMediaTime(id: string, time: number): Promise<void> {
  setWidgetMediaTimeLocal(id, time);
  await commitScene();
}

/// Change l'état lecture/pause du fond vidéo → commit (snapshot :4321).
export async function setBgPaused(paused: boolean): Promise<void> {
  sceneStore.update((s) => ({ ...s, bgPaused: paused }));
  await commitScene();
}

/// Maj locale de la position du fond vidéo (pendant le drag du slider seek).
/// PAS d'invoke — commit au release. Même règle que setWidgetMediaTimeLocal.
export function setBgTimeLocal(time: number): void {
  const t = Math.max(0, time);
  sceneStore.update((s) => ({ ...s, bgTime: t }));
}

/// Change la position (seek) du fond vidéo → commit (snapshot :4321).
export async function setBgTime(time: number): Promise<void> {
  setBgTimeLocal(time);
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
  if (!w) throw new Error(get(t)("scene.widgetIntrouvable"));
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
  if (!w) throw new Error(get(t)("scene.widgetIntrouvable"));
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
  if (!w) throw new Error(get(t)("scene.widgetIntrouvable"));
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

type SnapSide =
  | "left"
  | "center"
  | "right"
  | "top"
  | "bottom"
  | "mirror"
  | "gap";
export type SnapTarget = { pos: number; side: SnapSide };

/// Rect générique de cible de snap — widgets et modules HUD partagent les
/// mêmes champs x/y/largeur/hauteur.
type SnapRect = { x: number; y: number; largeur: number; hauteur: number };

/// Rects des modules HUD actifs — couche séparée des widgets mais cibles de
/// snap au même titre. Un module inactif n'est pas rendu → jamais cible.
/// `sauf` = module en cours de drag (exclu de ses propres cibles).
function rectsModulesActifs(scene: Scene, sauf?: ModuleId): SnapRect[] {
  const mods = scene.modules;
  if (!mods) return [];
  const out: SnapRect[] = [];
  for (const id of MODULE_IDS) {
    const m = mods[id];
    if (m?.actif && id !== sauf) {
      out.push({ x: m.x, y: m.y, largeur: m.largeur, hauteur: m.hauteur });
    }
  }
  return out;
}

/// Corps commun de construction des cibles : bords/centre canvas +
/// bords/centres des rects « autres » (widgets et/ou modules actifs) +
/// positions MIROIR (symétrie axe central : marge opposée égale à celle d'un
/// autre élément) + ESPACEMENTS ÉGAUX (le gap créé par l'élément déplacé
/// égale un gap existant entre deux autres éléments).
/// dw/dh = dims de l'élément déplacé (cible « centré entre deux » des gaps).
function snapTargetsDepuisRects(
  scene: Scene,
  autres: SnapRect[],
  dw: number,
  dh: number
): { xs: SnapTarget[]; ys: SnapTarget[] } {
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
  for (const o of autres) {
    xs.push({ pos: o.x, side: "left" });
    xs.push({ pos: o.x + o.largeur / 2, side: "center" });
    xs.push({ pos: o.x + o.largeur, side: "right" });
    ys.push({ pos: o.y, side: "top" });
    ys.push({ pos: o.y + o.hauteur / 2, side: "center" });
    ys.push({ pos: o.y + o.hauteur, side: "bottom" });
    // Miroir axe vertical : bord droit du déplacé → marge droite = marge
    // gauche de o ; bord gauche du déplacé → marge gauche = marge droite de o.
    xs.push({ pos: cw - o.x, side: "mirror" });
    xs.push({ pos: cw - o.x - o.largeur, side: "mirror" });
    // Miroir axe horizontal : idem en haut/bas.
    ys.push({ pos: ch - o.y, side: "mirror" });
    ys.push({ pos: ch - o.y - o.hauteur, side: "mirror" });
  }
  // Espacements égaux (smart guides) : pour chaque paire non chevauchante,
  // le widget déplacé peut prolonger la série au même gap, ou se centrer
  // entre les deux si la paire l'encadre.
  for (const a of autres) {
    for (const b of autres) {
      if (a === b) continue;
      const gapX = b.x - (a.x + a.largeur);
      if (gapX > 0) {
        xs.push({ pos: a.x - gapX, side: "gap" });
        xs.push({ pos: b.x + b.largeur + gapX, side: "gap" });
        if (dw > 0 && gapX > dw) {
          xs.push({ pos: a.x + a.largeur + (gapX - dw) / 2, side: "gap" });
        }
      }
      const gapY = b.y - (a.y + a.hauteur);
      if (gapY > 0) {
        ys.push({ pos: a.y - gapY, side: "gap" });
        ys.push({ pos: b.y + b.hauteur + gapY, side: "gap" });
        if (dh > 0 && gapY > dh) {
          ys.push({ pos: a.y + a.hauteur + (gapY - dh) / 2, side: "gap" });
        }
      }
    }
  }
  return { xs, ys };
}

/// Snapshot des cibles de snap pour le drag/resize d'un WIDGET.
/// Autres widgets + modules actifs comme cibles (réciproque : un widget
/// s'aligne aussi sur les modules HUD visibles).
/// Appelé UNE FOIS au pointerdown (pas par frame).
export function buildSnapTargets(scene: Scene, draggedId: string): {
  xs: SnapTarget[];
  ys: SnapTarget[];
} {
  const dragged = scene.widgets.find((w) => w.id === draggedId);
  const autres: SnapRect[] = [
    ...scene.widgets.filter((o) => o.id !== draggedId),
    ...rectsModulesActifs(scene),
  ];
  return snapTargetsDepuisRects(
    scene,
    autres,
    dragged?.largeur ?? 0,
    dragged?.hauteur ?? 0
  );
}

/// Snapshot des cibles de snap pour le drag/resize d'un MODULE HUD :
/// tous les widgets + les autres modules actifs (le module déplacé est
/// exclu de ses propres cibles via `sauf`).
/// Appelé UNE FOIS au pointerdown (pas par frame).
export function buildSnapTargetsModule(scene: Scene, moduleId: ModuleId): {
  xs: SnapTarget[];
  ys: SnapTarget[];
} {
  const dragged = scene.modules?.[moduleId];
  const autres: SnapRect[] = [
    ...scene.widgets,
    ...rectsModulesActifs(scene, moduleId),
  ];
  return snapTargetsDepuisRects(
    scene,
    autres,
    dragged?.largeur ?? 0,
    dragged?.hauteur ?? 0
  );
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

// ===== Cadre SVG unifié (widgets + bord du canvas) =====

const CADRE_DEFAUT: CadreConfig = {
  style: "carre",
  strokeWidth: 4,
  couleur: "#ffffff",
  couleurFin: "#000000",
  gradientAngle: 135,
  actif: false,
};

/// Cadre posé automatiquement à la création d'un widget quand la scène n'a
/// jamais eu de cadre configuré (voir sceneAvecWidget) : MGStyle, bordure
/// noire + accent violet marque StreamOS, épaisseur renforcée (50 px — test).
const CADRE_WIDGET_DEFAUT: CadreConfig = {
  ...CADRE_DEFAUT,
  style: "mgstyle",
  strokeWidth: 50,
  couleur: "#000000",
  couleurFin: "#8b5cf6",
  actif: true,
  bordActif: true,
};

/// Met à jour LE cadre de la scène (un seul système : widgets + bord canvas).
/// Écrit cadreWidget (champ canonique) et purge cadreApp (champ legacy —
/// migré par le Rust au chargement, plus jamais une vérité séparée).
/// `variante` (legacy presets) n'est plus écrit : purgé à chaque update.
export async function mettreAJourCadre(patch: Partial<CadreConfig>): Promise<void> {
  sceneStore.update((s) => {
    const cadreActuel = s.cadreWidget ?? { ...CADRE_WIDGET_DEFAUT };
    return { ...s, cadreWidget: { ...cadreActuel, ...patch, variante: undefined }, cadreApp: undefined };
  });
  await commitScene();
}
