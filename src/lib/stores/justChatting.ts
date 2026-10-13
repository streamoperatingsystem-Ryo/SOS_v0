// Store Just Chatting — overlay non-destructif : en diffusion, seuls la
// caméra et le(s) chat(s) restent visibles, repositionnés sur le layout JC
// (formule unique calculée en Rust, fractions 0–1 → × canvas réel).
//
// La scène persistée n'est JAMAIS mutée : pas de commitScene, pas de
// update_scene, pas de just_chatting.json. `actif` est un état runtime,
// OFF au boot — l'état revient par `just_chatting_etat` + event
// `just-chatting:etat` (source de vérité Rust, pattern ecranAttente.ts).
//
// OBS : le DOM masqué ne suffit pas — un trou destination-out non percé
// laisse la source SOS-Trou-* visible si le fond est transparent ou si le
// fond EST une capture (SOS-Fond-* plein canvas). Le toggle appelle donc
// `scene_sync_captures` avec une SCÈNE DÉRIVÉE en mémoire (copie complète :
// caméra + chats aux rects JC, autres widgets absents, bgObsSource vidé,
// modules HUD vidés) —
// le contrat unique obs_trou::sync_scene_captures désactive alors les items
// orphelins (jamais RemoveInput). La copie n'est jamais écrite : ni
// state.scene, ni disque.
import { writable, derived, get } from "svelte/store";
import { listen } from "@tauri-apps/api/event";
import {
  tauri,
  type JustChattingEtat,
  type JcLayout,
  type Scene,
} from "../tauri";
import { pointsClipPathCadre } from "../cadres/points";
import {
  sceneStore,
  syncCapturesOBS,
  syncCameraMask,
  registerApresSyncCaptures,
} from "./scene";
import { obsHost, obsPort, obsPassword } from "./obs";

/// Snapshot JC (actif + layouts des deux orientations). null = pas chargé.
export const justChattingStore = writable<JustChattingEtat | null>(null);

/// true = mode actif (runtime, OFF au boot). Dérivé du snapshot.
export const jcActif = derived(
  justChattingStore,
  ($e) => $e?.actif ?? false
);

let listenersBound = false;

/// Initialise le listener Tauri + le hook post-sync captures. Idempotent.
/// À appeler au mount du dashboard (initJustChatting + chargerJustChatting).
export async function initJustChatting(): Promise<void> {
  if (listenersBound) return;
  listenersBound = true;

  await listen<JustChattingEtat>("just-chatting:etat", (e) => {
    justChattingStore.set(e.payload);
  });

  // Changement de scène / preset / reconnexion OBS pendant que le mode est
  // actif : syncCapturesOBS vient de réactiver les trous de la NOUVELLE
  // scène → renvoyer la scène dérivée pour les re-cacher. Enregistré ici
  // (pas d'import scene.ts → justChatting.ts : sens unique, pas de cycle).
  registerApresSyncCaptures(async () => {
    if (get(jcActif)) {
      await syncObsJC();
    }
  });
}

/// Charge l'état initial (OFF au boot — actif n'est pas persisté).
export async function chargerJustChatting(): Promise<void> {
  try {
    justChattingStore.set(await tauri.justChattingEtat());
  } catch {
  }
}

// =============================================================================
// OBS — scène dérivée + caméra au rect JC
// =============================================================================

/// Creds OBS des stores (localStorage — partagés avec la modale OBS).
function obsCreds(): { host: string; port: number; password: string } {
  return {
    host: get(obsHost),
    port: parseInt(get(obsPort), 10) || 4455,
    password: get(obsPassword),
  };
}

/// Layout JC courant selon l'orientation du canvas réel (paysage si W >= H).
/// null = état pas encore chargé.
function layoutCourant(scene: Scene): JcLayout | null {
  const e = get(justChattingStore);
  if (!e) return null;
  const w = scene.canvasW ?? 1920;
  const h = scene.canvasH ?? 1080;
  return w >= h ? e.paysage : e.portrait;
}

/// Scène DÉRIVÉE complète (copie en mémoire, jamais persistée) : caméra et
/// chat(s) repositionnés aux rects JC en pixels canvas, autres widgets
/// absents, bgObsSource vidé (sinon la capture plein canvas resterait
/// affichée), modules HUD vidés (parité avec la dérivation diffusion —
/// horloge/musique/objectifs/équaliseur masqués pendant JC). canvasW/H
/// inchangés — le contrat serde est celui d'une scène complète, pas un
/// patch. Même résultat que la dérivation de diffusion.html
/// (jcDeriverScene) — ici elle sert à la sync OBS.
export function sceneDerivee(scene: Scene): Scene | null {
  const l = layoutCourant(scene);
  if (!l) return null;
  const cw = scene.canvasW ?? 1920;
  const ch = scene.canvasH ?? 1080;
  const widgets = scene.widgets
    .filter((w) => w.type === "camera" || w.type === "chat")
    .map((w) => {
      const r = w.type === "camera" ? l.camera : l.chat;
      return {
        ...w,
        x: r.x * cw,
        y: r.y * ch,
        largeur: r.w * cw,
        hauteur: r.h * ch,
      };
    });
  return { ...scene, widgets, bgObsSource: undefined, modules: {} };
}

/// Sync OBS du mode actif : scène dérivée (sync_scene_captures désactive
/// les SOS-Trou-* absents et tout SOS-Fond-* puisque bgObsSource est vidé,
/// et cale la caméra sur le rect JC via ensure_camera), puis cameraSync
/// explicite + masque aux dims JC (le PNG du masque dépend de la taille du
/// widget — sync_scene_captures cale le transform, pas le masque).
///
/// Appelle les wrappers Tauri DIRECTEMENT — jamais syncCapturesOBS (elle
/// enchaîne les hooks enregistrés, dont celui-ci → boucle). Non-fatal OBS
/// off : auto-guérison au prochain toggle ou sync captures.
async function syncObsJC(): Promise<void> {
  const scene = get(sceneStore);
  const l = layoutCourant(scene);
  const derivee = sceneDerivee(scene);
  if (!l || !derivee) return;
  const { host, port, password } = obsCreds();
  try {
    await tauri.sceneSyncCaptures(host, port, password, derivee);
  } catch {
    // OBS offline → réessayé au prochain toggle/sync.
  }
  const cam = scene.widgets.find((w) => w.type === "camera");
  if (!cam) return; // pas de widget caméra → skip cameraSync + masque
  const cw = scene.canvasW ?? 1920;
  const ch = scene.canvasH ?? 1080;
  const jcW = l.camera.w * cw;
  const jcH = l.camera.h * ch;
  try {
    // device = null : ne pas toucher aux settings d'un input existant.
    await tauri.cameraSync(
      host, port, password, null,
      l.camera.x * cw, l.camera.y * ch, jcW, jcH
    );
  } catch {
    // OBS offline → auto-guérison au prochain scene_sync_captures.
  }
  try {
    // Mêmes conditions de forme que pointsMasqueCamera (cadre actif +
    // strokeWidth > 0 + pas de sansCadre), dims = rect JC.
    const cadre = scene.cadreWidget;
    const points =
      cam.sansCadre === true || !cadre?.actif || (cadre.strokeWidth || 0) <= 0
        ? null
        : pointsClipPathCadre(cadre.style, jcW, jcH, cadre.strokeWidth || 4);
    await tauri.cameraMaskSync(
      host, port, password, points, jcW, jcH, "remplir", 1
    );
  } catch {
    // OBS offline → retente au prochain toggle.
  }
}

// =============================================================================
// ACTIONS
// =============================================================================

/// Active ou coupe le mode. Bascule Rust (frame WS + event) puis sync OBS.
export async function setJustChatting(actif: boolean): Promise<void> {
  await tauri.justChattingSetActif(actif);
  // Rafraîchit le store AVANT les syncs : le hook post-sync lit jcActif, il
  // doit refléter la nouvelle valeur (l'event Rust reviendra — idempotent).
  await chargerJustChatting();
  if (actif) {
    // ON : scène dérivée → caméra/chats seuls côté OBS + caméra au rect JC.
    await syncObsJC();
  } else {
    // OFF : sync captures de la scène RÉELLE (réactive trous + fond, recale
    // la caméra sur le widget jamais muté) puis masque FORCÉ — le cache de
    // syncCameraMask détient encore la clé réelle alors que le PNG OBS est
    // aux dims JC → sans forcer, le rétablissement serait sauté.
    await syncCapturesOBS();
    await syncCameraMask(true);
  }
}

/// Toggle — pour la Toolbar, la pastille topbar et la barre de raccourcis.
export async function toggleJustChatting(): Promise<void> {
  await setJustChatting(!get(jcActif));
}
