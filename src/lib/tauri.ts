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
}

export interface Scene {
  widgets: Widget[];
}

export const tauri = {
  async getScene(): Promise<Scene> {
    return invoke<Scene>("get_scene");
  },

  async updateScene(scene: Scene): Promise<void> {
    await invoke("update_scene", { scene });
  },

  /// Importe une image pour un widget. Retourne le chemin relatif
  /// ("medias/<uuid>.<ext>") ou null si le dialog a été annulé.
  /// Lance une erreur en cas de refus (taille/format).
  async importMedia(widgetId: string): Promise<string | null> {
    return invoke<string | null>("import_media", { widgetId });
  },

  /// Connecte à OBS WebSocket, authentifie, s'assure que la scène "SOS"
  /// + source navigateur "SOS" (1920×1080, URL :4321) existent sans doublon.
  async obsConnect(host: string, port: number, password: string): Promise<void> {
    await invoke("obs_connect", { host, port, password });
  },
};
