// Store du fond d'image du dashboard entier.
// Image choisie par l'utilisateur (Personnalisation → « Image de fond »),
// importée via le pipeline média commun (validate_and_copy_media → medias/,
// servie par :4321) puis affichée en pleine fenêtre sous tout le chrome
// (.fond-image dans .fond-decor — app.css ; le canvas reste opaque).
// Persistée en localStorage (préférence cosmétique machine-only, même
// pattern que obs.ts).
import { get, writable, type Writable } from "svelte/store";
import { tauri } from "../tauri";

const LS_FOND = "sidebarFond";
const MEDIA_BASE = "http://127.0.0.1:4321/";

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

/// Vérifie au boot que le média du fond existe encore sur :4321 — sinon
/// reset du store (localStorage nettoyé via le subscribe ci-dessus). Le
/// fichier peut avoir été supprimé du dossier medias/ hors de l'app :
/// localStorage garderait la référence et la CSS redemanderait l'URL à
/// chaque rendu (404 en boucle). Jamais de reset sur erreur réseau pure
/// (serveur :4321 pas encore prêt, offline) : on attend une vraie réponse
/// HTTP, bornée ~15 s puis abandon (le fond reste, tant pis).
async function verifierFondExistant(rel: string | null): Promise<void> {
  if (!rel) return;
  for (let i = 0; i < 15; i++) {
    try {
      const r = await fetch(`${MEDIA_BASE}${rel}`, { method: "HEAD" });
      if (!r.ok) fondSidebar.set(null);
      return;
    } catch {
      await new Promise((res) => setTimeout(res, 1000));
    }
  }
}

void verifierFondExistant(get(fondSidebar));

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
