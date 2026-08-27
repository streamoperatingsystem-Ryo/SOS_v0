// Registre des <video> du dashboard : id widget (ou "fond") → élément.
// Sert uniquement à lire la duration (loadedmetadata) pour la barre lecteur.
// La barre lecteur NE pilote PAS le dashboard — elle écrit mediaPaused/mediaTime
// dans la scène (commitScene → snapshot WS → :4321 applique play/pause/seek).
import { writable } from "svelte/store";

export const videoRegistry = writable<Map<string, HTMLVideoElement>>(new Map());

/// Enregistre une vidéo dans le registre (id widget ou "fond").
export function registerVideo(id: string, el: HTMLVideoElement): void {
  videoRegistry.update((m) => {
    const n = new Map(m);
    n.set(id, el);
    return n;
  });
}

/// Retire une vidéo du registre (au unmount / changement de src).
export function unregisterVideo(id: string): void {
  videoRegistry.update((m) => {
    const n = new Map(m);
    n.delete(id);
    return n;
  });
}
