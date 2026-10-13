// Store musique côté dashboard. "!rs <titre>" tapé par un viewer → file de
// lecture de la bibliothèque LOCALE (medias/musique/), audio joué côté
// diffusion :4321. La détection est côté Rust (hooks chat) — le dashboard
// gère la config, la bibliothèque, la file d'attente et les contrôles live.
// L'état est poussé par l'event Tauri "musique:update" (état complet).
import { writable } from "svelte/store";
import { listen } from "@tauri-apps/api/event";
import { tauri, type MusiqueEtat, type MusiqueConfig } from "../tauri";

/// État complet : config + piste en cours + file + bibliothèque.
/// null tant que musique_etat n'a jamais répondu (modal fermée au boot).
export const musiqueEtat = writable<MusiqueEtat | null>(null);

/// Bandes de l'équalizer (16 valeurs 0-255) — poussées par l'event
/// "musique:spectrum" relayé par Rust depuis l'AnalyserNode de la
/// diffusion (~8 Hz). Le lecteur retombe sur une animation simulée si
/// aucune frame n'arrive pendant la lecture (PREVIEW, ctx suspendu).
export const musiqueBands = writable<number[]>(new Array(16).fill(0));

/// Timestamp (ms) de la dernière frame spectrum reçue — permet au lecteur
/// de détecter l'absence de frames et de passer en animation simulée.
export const musiqueBandsTs = writable<number>(0);

let initialized = false;

/// Enregistre le listener "musique:update" UNE FOIS + charge l'état initial.
/// À appeler à l'ouverture de l'onglet Musique (idempotent).
export async function initMusique(): Promise<void> {
  if (initialized) return;
  initialized = true;
  await listen<MusiqueEtat>("musique:update", (e) => musiqueEtat.set(e.payload));
  await listen<{ id?: string; bands?: number[] }>(
    "musique:spectrum",
    (e) => {
      const bands = e.payload?.bands;
      if (Array.isArray(bands) && bands.length > 0) {
        musiqueBands.set(bands);
        musiqueBandsTs.set(Date.now());
      }
    }
  );
  await chargerMusique();
}

/// Recharge l'état complet depuis Rust (état = source de vérité Rust).
export async function chargerMusique(): Promise<void> {
  try {
    musiqueEtat.set(await tauri.musiqueEtat());
  } catch (e) {
    console.error("chargerMusique:", e);
  }
}

/// Sauvegarde la config (le Rust normalise les noms + emit update).
export async function sauverConfigMusique(
  cfg: MusiqueConfig
): Promise<boolean> {
  try {
    await tauri.musiqueSetConfig(cfg);
    return true;
  } catch (e) {
    console.error("sauverConfigMusique:", e);
    return false;
  }
}

/// Importe des fichiers audio (dialog multi-sélection) → medias/musique/.
export async function importerMusique(): Promise<void> {
  try {
    await tauri.musiqueImporter();
  } catch (e) {
    console.error("importerMusique:", e);
  }
}

/// Supprime un fichier de la bibliothèque (nom de fichier, pas un src).
export async function supprimerFichierMusique(nom: string): Promise<void> {
  try {
    await tauri.musiqueSupprimerFichier(nom);
  } catch (e) {
    console.error("supprimerFichierMusique:", e);
  }
}

/// Skip la piste courante → la suivante démarre.
export async function musiqueSkip(): Promise<void> {
  try {
    await tauri.musiqueSkip();
  } catch (e) {
    console.error("musiqueSkip:", e);
  }
}

/// Pause/reprendre la piste courante côté diffusion.
export async function musiquePause(pause: boolean): Promise<void> {
  try {
    await tauri.musiquePause(pause);
  } catch (e) {
    console.error("musiquePause:", e);
  }
}

/// Stoppe la lecture ET vide la file.
export async function musiqueStop(): Promise<void> {
  try {
    await tauri.musiqueStop();
  } catch (e) {
    console.error("musiqueStop:", e);
  }
}

/// Vide la file (la piste courante continue).
export async function musiqueVider(): Promise<void> {
  try {
    await tauri.musiqueVider();
  } catch (e) {
    console.error("musiqueVider:", e);
  }
}

/// Retire une entrée de la file par id.
export async function musiqueRetirer(id: string): Promise<void> {
  try {
    await tauri.musiqueRetirer(id);
  } catch (e) {
    console.error("musiqueRetirer:", e);
  }
}

/// Déplace une entrée dans la file (-1 monter / +1 descendre).
export async function musiqueDeplacer(id: string, delta: number): Promise<void> {
  try {
    await tauri.musiqueDeplacer(id, delta);
  } catch (e) {
    console.error("musiqueDeplacer:", e);
  }
}

/// Ajoute des fichiers de la bibliothèque à la file (sélection streamer —
/// sans limites viewer, en fin de file ; 1re piste direct si rien ne joue).
/// Retourne le nombre de pistes ajoutées (null = erreur).
export async function ajouterFileMusique(srcs: string[]): Promise<number | null> {
  try {
    return await tauri.musiqueFileAjouter(srcs);
  } catch (e) {
    console.error("ajouterFileMusique:", e);
    return null;
  }
}

/// Joue un fichier de la bibliothèque sur la diffusion (test manuel).
export async function testerMusique(src: string): Promise<void> {
  try {
    await tauri.musiqueTester(src);
  } catch (e) {
    console.error("testerMusique:", e);
  }
}

/// Change le volume côté diffusion (persisté en config).
export async function musiqueVolume(volume: number): Promise<void> {
  try {
    await tauri.musiqueVolume(volume);
  } catch (e) {
    console.error("musiqueVolume:", e);
  }
}
