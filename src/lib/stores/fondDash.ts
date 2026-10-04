// Store du fond d'image du chrome dashboard (topbar + sidebar + zones
// transparentes autour du canvas). Distinct de fondSidebar.ts (décor
// d'ambiance derrière toute la fenêtre) : ici l'image est posée en
// background de <main> — elle transparaît dans le chrome transparent et
// le .canvas (background: var(--fond), opaque) la recouvre → la
// composition n'est jamais touchée.
// Image choisie via le bouton « Fond du dashboard » en bas du panneau
// latéral, importée par le pipeline média commun (validate_and_copy_media
// → medias/, servie par :4321).
// Persistée en localStorage (préférence cosmétique machine-only, même
// pattern que fondSidebar.ts / obs.ts).
import { writable, type Writable } from "svelte/store";
import { tauri } from "../tauri";

const LS_FOND = "dashFond";

function lireLS(cle: string): string | null {
  try {
    return localStorage.getItem(cle);
  } catch {
    return null; // localStorage indisponible (webview restreinte)
  }
}

/// Chemin relatif ("medias/<uuid>.<ext>") de l'image de fond du chrome, ou null.
export const fondDash: Writable<string | null> = writable(lireLS(LS_FOND));

fondDash.subscribe((v) => {
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
export async function importerFondDash(): Promise<void> {
  const rel = await tauri.importAlerteMedia("image");
  if (rel) fondDash.set(rel);
}

export function retirerFondDash(): void {
  fondDash.set(null);
}
