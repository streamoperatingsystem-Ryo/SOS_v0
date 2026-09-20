// Store du fond d'image du dashboard entier.
// Image choisie par l'utilisateur (Personnalisation → « Image de fond »),
// importée via le pipeline média commun (validate_and_copy_media → medias/,
// servie par :4321) puis affichée en pleine fenêtre sous tout le chrome
// (.fond-image dans .fond-decor — app.css ; le canvas reste opaque).
// Persistée en localStorage (préférence cosmétique machine-only, même
// pattern que obs.ts).
import { writable, type Writable } from "svelte/store";
import { tauri } from "../tauri";

const LS_FOND = "sidebarFond";

function lireLS(cle: string): string | null {
  try {
    return localStorage.getItem(cle);
  } catch {
    return null; // localStorage indisponible (webview restreinte)
  }
}

/// Chemin relatif ("medias/<uuid>.<ext>") de l'image de fond, ou null.
export const fondSidebar: Writable<string | null> = writable(lireLS(LS_FOND));

fondSidebar.subscribe((v) => {
  try {
    if (v) localStorage.setItem(LS_FOND, v);
    else localStorage.removeItem(LS_FOND);
  } catch {
    // ignore (quota / mode privé) — le fond marche quand même en RAM
  }
});

/// Dialog « Images » → validation + copie vers medias/ (commande commune
/// import_alerte_media, filtre image seul) → le fond suit immédiatement.
/// Dialog annulé → rien ne change. Les erreurs remontent à l'appelant.
export async function importerFondSidebar(): Promise<void> {
  const rel = await tauri.importAlerteMedia("image");
  if (rel) fondSidebar.set(rel);
}

export function retirerFondSidebar(): void {
  fondSidebar.set(null);
}
