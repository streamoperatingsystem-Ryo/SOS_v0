// Store pad numérique : état config + listeners events Rust + audio local.
//
// Pad numérique = 16 touches Numpad × 3 plages, déclenchement sons/images/vidéos
// sur touche pressée. Porté depuis l'ancienne app RUST_SOS_2026 (features/
// pad-numerique/), réécrit pour l'architecture v0.
//
// Flux :
//   - Rust capture clavier globale (GetAsyncKeyState) → emit "pad:touche"
//     → ce store joue l'audio LOCALEMENT (le user entend sur son PC).
//   - Rust émet aussi "pad-play"/"pad-stop" sur le WS :4321 → diffusion.html
//     affiche l'image/vidéo + joue le son (OBS capte pour les viewers).
//   - "pad:etat" : config changée (touche modifiée, plage changée, actif).
//   - "pad:plage-changee" : plage changée via touches + ou - du numpad.
//   - "pad:touche-stop" : le timer Rust a expiré → stop audio local.
import { writable } from "svelte/store";
import { listen } from "@tauri-apps/api/event";
import { tauri, type PadConfig, type ToucheConfig } from "../tauri";

// =============================================================================
// CONSTANTES (identiques à l'ancienne app, mirroir de pad_numerique.rs)
// =============================================================================

/// Disposition du pad numérique (4 colonnes × 4 lignes).
export const DISPOSITION_PAD: string[][] = [
  ["Numpad7", "Numpad8", "Numpad9", "NumpadDivide"],
  ["Numpad4", "Numpad5", "Numpad6", "NumpadMultiply"],
  ["Numpad1", "Numpad2", "Numpad3", "NumpadSubtract"],
  ["Numpad0", "NumpadDecimal", "NumpadEnter", "NumpadAdd"],
];

/// Touches réservées (non assignables, navigation entre plages).
export const TOUCHES_RESERVEES: string[] = ["NumpadAdd", "NumpadSubtract"];

/// Codes de touches assignables (exclut + et - qui sont réservés).
export const CODES_ASSIGNABLES: string[] = [
  "Numpad0", "Numpad1", "Numpad2", "Numpad3", "Numpad4",
  "Numpad5", "Numpad6", "Numpad7", "Numpad8", "Numpad9",
  "NumpadDecimal", "NumpadEnter",
  "NumpadMultiply", "NumpadDivide",
];

/// Noms d'affichage des touches (pour l'UI).
export const NOMS_AFFICHAGE: Record<string, string> = {
  Numpad0: "0",
  Numpad1: "1",
  Numpad2: "2",
  Numpad3: "3",
  Numpad4: "4",
  Numpad5: "5",
  Numpad6: "6",
  Numpad7: "7",
  Numpad8: "8",
  Numpad9: "9",
  NumpadDecimal: ".",
  NumpadEnter: "Enter",
  NumpadMultiply: "*",
  NumpadDivide: "/",
  NumpadAdd: "+",
  NumpadSubtract: "-",
};

/// Libellés des touches réservées (navigation plages).
export const LIBELLES_RESERVEES: Record<string, string> = {
  NumpadAdd: "Plage suivante",
  NumpadSubtract: "Plage précédente",
};

/// Nombre de plages (0, 1, 2).
export const NB_PLAGES = 3;

// =============================================================================
// STORE
// =============================================================================

export const padStore = writable<PadConfig | null>(null);

/// Erreur/avertissement pad (ex : NumLock OFF). null = pas d'erreur.
export const padErreur = writable<string | null>(null);

let listenersBound = false;

/// Audio local courant (joué côté dashboard pour le user).
let audioLocal: HTMLAudioElement | null = null;

/// Initialise les listeners Tauri. Idempotent. À appeler au mount du dashboard.
export async function initPad(): Promise<void> {
  if (listenersBound) return;
  listenersBound = true;

  // Config changée (touche modifiée, plage changée, actif).
  await listen<PadConfig>("pad:etat", (e) => {
    padStore.set(e.payload);
  });

  // Touche déclenchée (par capture clavier OU bouton Test) → joue audio local.
  await listen<{
    code: string;
    plage: number;
    media_type: string;
    media?: string;
    son?: string;
    volume: number;
    duree_ms: number;
  }>("pad:touche", (e) => {
    jouerAudioLocal(e.payload);
  });

  // Timer Rust expiré → stop audio local.
  await listen<{ code: string; plage: number }>("pad:touche-stop", () => {
    stopperAudioLocal();
  });

  // Plage changée via touches + ou - du numpad.
  await listen<{ plage: number }>("pad:plage-changee", (e) => {
    padStore.update((p) => (p ? { ...p, plage_actuelle: e.payload.plage } : p));
  });

  // Avertissement NumLock OFF (ou null quand NumLock repasse ON).
  await listen<string | null>("pad:erreur", (e) => {
    padErreur.set(e.payload);
  });

  console.log("[PadNumérique] listeners bound");
}

/// Charge l'état initial du pad. À appeler après initPad.
export async function chargerPad(): Promise<void> {
  try {
    const etat = await tauri.padEtat();
    padStore.set(etat);
  } catch (e) {
    console.error("[PadNumérique] chargerPad ERR:", String(e));
  }
}

// =============================================================================
// ACTIONS (wrappers tauri → refresh état)
// =============================================================================

async function refresh(): Promise<void> {
  try {
    const etat = await tauri.padEtat();
    padStore.set(etat);
  } catch (e) {
    console.error("[PadNumérique] refresh ERR:", String(e));
  }
}

export async function padSetActif(actif: boolean): Promise<void> {
  await tauri.padSetActif(actif);
  await refresh();
}

export async function padSetTouche(
  code: string,
  plage: number,
  config: ToucheConfig,
): Promise<void> {
  await tauri.padSetTouche(code, plage, config);
  await refresh();
}

export async function padSupprimerTouche(code: string, plage: number): Promise<void> {
  await tauri.padSupprimerTouche(code, plage);
  await refresh();
}

export async function padSetPlage(plage: number): Promise<void> {
  await tauri.padSetPlage(plage);
  await refresh();
}

export async function padTesterTouche(code: string, plage: number): Promise<void> {
  await tauri.padTesterTouche(code, plage);
}

/// Importe un média (image OU vidéo) pour une touche. Retourne le chemin
/// relatif ("medias/<uuid>.<ext>") ou null si dialog annulé.
export async function padImporterMedia(
  code: string,
  plage: number,
  kind: "image" | "video",
): Promise<string | null> {
  try {
    const rel = await tauri.padImporterMedia(kind);
    if (rel) {
      // Lire la touche courante pour préserver les autres champs.
      const cfg = await tauri.padEtat();
      const cle = `${code}_${plage}`;
      const touche = cfg.touches[cle] ?? {};
      await padSetTouche(code, plage, {
        ...touche,
        media_type: kind,
        media: rel,
        media_kind: kind,
      });
    }
    return rel;
  } catch (e) {
    console.error("[PadNumérique] padImporterMedia ERR:", String(e));
    throw e;
  }
}

/// Importe un son pour une touche. Retourne le chemin relatif ou null.
export async function padImporterSon(
  code: string,
  plage: number,
): Promise<string | null> {
  try {
    const rel = await tauri.padImporterSon();
    if (rel) {
      const cfg = await tauri.padEtat();
      const cle = `${code}_${plage}`;
      const touche = cfg.touches[cle] ?? {};
      // Si la touche n'a pas de média, c'est un audio pur. Sinon, c'est le son
      // séparé d'une image.
      const estAudioPur = !touche.media;
      await padSetTouche(code, plage, {
        ...touche,
        media_type: estAudioPur ? "audio" : touche.media_type,
        media: estAudioPur ? rel : touche.media,
        son: estAudioPur ? undefined : rel,
      });
    }
    return rel;
  } catch (e) {
    console.error("[PadNumérique] padImporterSon ERR:", String(e));
    throw e;
  }
}

// =============================================================================
// AUDIO LOCAL (dashboard — le user entend sur son PC)
// =============================================================================

/// Joue l'audio local pour une touche déclenchée. Pour "audio" pur → joue le
/// média. Pour "image" avec son → joue le son séparé. Pour "video" → ne joue
/// rien côté dashboard (le son est porté par la vidéo côté diffusion).
function jouerAudioLocal(payload: {
  media_type: string;
  media?: string;
  son?: string;
  volume: number;
}): void {
  stopperAudioLocal();

  let src: string | undefined;
  if (payload.media_type === "audio") {
    src = payload.media;
  } else if (payload.media_type === "image" && payload.son) {
    src = payload.son;
  }
  // "video" : pas d'audio local (son porté par la vidéo côté diffusion).

  if (!src) return;

  // Construire l'URL : les médias sont servis par le serveur :4321.
  const url = src.startsWith("/") ? src : `/${src}`;
  const audio = new Audio(url);
  audio.volume = Math.max(0, Math.min(1, payload.volume));
  audio.play().catch((e) => {
    console.warn("[PadNumérique] audio local play échoué:", String(e));
  });
  audioLocal = audio;
}

/// Stoppe l'audio local courant (timer Rust expiré ou nouvelle touche).
function stopperAudioLocal(): void {
  if (audioLocal) {
    audioLocal.pause();
    audioLocal.currentTime = 0;
    audioLocal = null;
  }
}

/// Remet à zéro (après déconnexion).
export function resetPad(): void {
  padStore.set(null);
  stopperAudioLocal();
}
