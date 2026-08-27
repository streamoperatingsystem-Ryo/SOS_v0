// SEUL endroit où invoke/listen Tauri sont utilisés côté dashboard.
import { invoke } from "@tauri-apps/api/core";

export interface Widget {
  id: string;
  type: "media";
  x: number;
  y: number;
  largeur: number;
  hauteur: number;
  z: number;
  media?: string;
  rotateX?: number;
  rotateY?: number;
  mediaFit?: string;
  kind?: string;
  mediaZoom?: number;
  mediaRot?: number;
  mediaPaused?: boolean;
  mediaTime?: number;
  trou?: boolean;
}

export interface Scene {
  widgets: Widget[];
  canvasW?: number;
  canvasH?: number;
  bgMedia?: string;
  bgKind?: string;
  bgFit?: string;
  bgZoom?: number;
  bgRot?: number;
  bgPaused?: boolean;
  bgTime?: number;
}

/// Entrée de l'index des scènes (id + nom seulement, jamais le contenu).
export interface SceneIndex {
  id: string;
  nom: string;
}

export const tauri = {
  async getScene(): Promise<Scene> {
    return invoke<Scene>("get_scene");
  },

  async updateScene(scene: Scene): Promise<void> {
    await invoke("update_scene", { scene });
  },

  /// Importe un média (image OU vidéo) pour un widget. Retourne le chemin
  /// relatif ("medias/<uuid>.<ext>") ou null si le dialog a été annulé.
  /// Lance une erreur en cas de refus (taille/format). Le kind
  /// ("image"|"video") est déduit côté TS de l'extension du chemin retourné.
  async importMedia(widgetId: string): Promise<string | null> {
    return invoke<string | null>("import_media", { widgetId });
  },

  /// Importe un média (image OU vidéo) comme fond de scène. Retourne le
  /// chemin relatif ("medias/<uuid>.<ext>") ou null si dialog annulé.
  /// Mêmes limites que importMedia (helper commun Rust). Le kind est déduit
  /// côté TS de l'extension du chemin retourné.
  async importFond(): Promise<string | null> {
    return invoke<string | null>("import_fond");
  },

  /// Connecte à OBS WebSocket, authentifie, lit la résolution canvas OBS
  /// (GetVideoSettings → baseWidth/baseHeight, fallback 1920×1080), s'assure
  /// que la scène "SOS" + source navigateur "SOS-Diffusion" (dims = résolution
  /// OBS, URL :4321) existent sans doublon, puis mute la scène + save + snapshot.
  async obsConnect(host: string, port: number, password: string): Promise<void> {
    await invoke("obs_connect", { host, port, password });
  },

  // ===== Scènes (v0.14) =====

  /// Liste l'index des scènes (ids + noms seulement, jamais le contenu).
  async scenesLister(): Promise<SceneIndex[]> {
    return invoke<SceneIndex[]>("scenes_lister");
  },

  /// Crée une nouvelle scène vide (1920×1080, 0 widget, pas de fond), bascule
  /// dessus. Sauve la courante d'abord. Retourne l'entrée créée.
  async sceneCreer(nom: string): Promise<SceneIndex> {
    return invoke<SceneIndex>("scene_creer", { nom });
  },

  /// Ouvre une scène : sauve la courante, charge <id>.json, bascule, snapshot.
  async sceneOuvrir(id: string): Promise<SceneIndex> {
    return invoke<SceneIndex>("scene_ouvrir", { id });
  },

  /// Renomme une scène dans l'index.
  async sceneRenommer(id: string, nom: string): Promise<void> {
    await invoke("scene_renommer", { id, nom });
  },

  /// Retourne l'entrée courante ({ id, nom }).
  async sceneCourante(): Promise<SceneIndex> {
    return invoke<SceneIndex>("scene_courante");
  },

  /// Exporte la scène courante comme pack dossier portable.
  /// nom_pack = nom du dossier à créer (fourni par l'UI, défaut = nom scène).
  /// Dialog = dossier PARENT. Retourne true si exporté, false si annulé.
  async sceneExporter(nomPack: string): Promise<boolean> {
    return invoke<boolean>("scene_exporter", { nomPack });
  },

  /// Importe une scène depuis un .json (dialog Ouvrir). Valide schéma min,
  /// copie en <nouvel-id>.json, bascule. Retourne l'entrée ou null si annulé.
  async sceneImporter(): Promise<SceneIndex | null> {
    return invoke<SceneIndex | null>("scene_importer");
  },
};
