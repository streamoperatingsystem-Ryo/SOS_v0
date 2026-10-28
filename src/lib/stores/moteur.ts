// Registre du moteur de diffusion (lot 1.5) — mode OBS ou natif.
//
// L'état vient du backend (`moteur_etat`), jamais d'une supposition : les
// capacités affichées sont celles du moteur réellement actif. Les deux
// moteurs sont branchés depuis le lot 1.4 (natif = capture/composition/
// encodage sans OBS, audio loopback + micro muxé).
import { writable, get } from "svelte/store";
import { tauri } from "../tauri";
import { obsHost, obsPort, obsPassword } from "./obs";

export type ModeMoteur = "obs" | "natif";

export interface EtatMoteur {
  mode: ModeMoteur;
  natif_disponible: boolean;
  obs_disponible: boolean;
  capacites: string[];
}

/// État de la sortie native — reflète le backend, rien n'est inventé :
/// `chemin` vaut `null` hors enregistrement, `encodeur` reste `null` tant
/// que rien n'a démarré, `erreur` porte l'erreur terminale du moteur.
export interface SortieNatif {
  actif: boolean;
  chemin: string | null;
  kbps: number;
  frames_perdues: number;
  encodeur: string | null;
  degradation: string | null;
  erreur: string | null;
}

/// Défaut honnête : OBS, aucune capacité inventée.
export const moteurEtat = writable<EtatMoteur>({
  mode: "obs",
  natif_disponible: false,
  obs_disponible: true,
  capacites: [],
});

export const moteurErreur = writable<string | null>(null);

export const sortieNatif = writable<SortieNatif>({
  actif: false,
  chemin: null,
  kbps: 0,
  frames_perdues: 0,
  encodeur: null,
  degradation: null,
  erreur: null,
});

/// Opération de sortie en cours (démarrage GPU ou arrêt + muxage) : l'UI
/// désactive le bouton sans bloquer la fenêtre.
export const sortieNatifBusy = writable(false);

/// Dernier fichier écrit — conservé après l'arrêt pour l'afficher.
export const sortieNatifFichier = writable<string | null>(null);

let suivi: ReturnType<typeof setInterval> | null = null;

function arreterSuivi() {
  if (suivi !== null) {
    clearInterval(suivi);
    suivi = null;
  }
}

/// Suivi 1 s pendant l'enregistrement — lecture asynchrone, jamais bloquante.
function demarrerSuivi() {
  if (suivi !== null) return;
  suivi = setInterval(() => void rafraichirSortieNatif(), 1000);
}

/// Relit l'état du registre. Non-fatal : conserve le dernier état connu.
export async function chargerEtatMoteur(): Promise<void> {
  try {
    const e = await tauri.moteurEtat();
    moteurEtat.set({
      mode: e.mode === "natif" ? "natif" : "obs",
      natif_disponible: e.natif_disponible,
      obs_disponible: e.obs_disponible,
      capacites: e.capacites ?? [],
    });
  } catch (e) {
    moteurErreur.set(String(e));
  }
}

/// Transmet la connexion obs-websocket au backend (mémoire seule).
/// À appeler après une connexion OBS réussie ou au chargement de l'UI.
export async function envoyerConnexionObs(): Promise<void> {
  try {
    await tauri.moteurObsConnexion(
      get(obsHost),
      parseInt(get(obsPort), 10) || 4455,
      get(obsPassword),
    );
  } catch {
    // Non fatal : l'adaptateur refusera proprement ses actions.
  }
}

/// Bascule de mode. `liveActif` = OBS diffuse ou enregistre : le backend
/// refuse alors par défaut (jamais de coupure implicite), de même qu'une
/// sortie native active. Les capacités sont relues après la bascule.
export async function basculerMoteur(cible: ModeMoteur, liveActif: boolean): Promise<boolean> {
  moteurErreur.set(null);
  try {
    const mode = await tauri.moteurBasculer(cible, liveActif);
    moteurEtat.update((e) => ({ ...e, mode: mode === "natif" ? "natif" : "obs" }));
    await chargerEtatMoteur();
    return true;
  } catch (e) {
    moteurErreur.set(String(e));
    return false;
  }
}

/// Relit l'état de la sortie native. Le suivi périodique ne tourne que
/// pendant l'enregistrement (aucun timer au repos).
export async function rafraichirSortieNatif(): Promise<void> {
  try {
    const e = await tauri.moteurSortieEtat();
    sortieNatif.set({
      actif: e.actif,
      chemin: e.chemin ?? null,
      kbps: e.kbps ?? 0,
      frames_perdues: e.frames_perdues ?? 0,
      encodeur: e.encodeur ?? null,
      degradation: e.degradation ?? null,
      erreur: e.erreur ?? null,
    });
    if (e.chemin) sortieNatifFichier.set(e.chemin);
    if (e.actif) demarrerSuivi();
    else arreterSuivi();
  } catch (e) {
    arreterSuivi();
    moteurErreur.set(String(e));
  }
}

/// Démarre l'enregistrement natif vers `chemin` (choisi par l'utilisateur).
export async function demarrerEnregistrementNatif(chemin: string): Promise<boolean> {
  moteurErreur.set(null);
  sortieNatifBusy.set(true);
  try {
    const fichier = await tauri.moteurSortieDemarrer(chemin);
    sortieNatifFichier.set(fichier);
    await rafraichirSortieNatif();
    return true;
  } catch (e) {
    moteurErreur.set(String(e));
    return false;
  } finally {
    sortieNatifBusy.set(false);
  }
}

/// Arrête l'enregistrement natif, finalise le fichier et muxe l'audio.
export async function arreterEnregistrementNatif(): Promise<string | null> {
  moteurErreur.set(null);
  sortieNatifBusy.set(true);
  try {
    const fichier = await tauri.moteurSortieArreter();
    if (fichier) sortieNatifFichier.set(fichier);
    await rafraichirSortieNatif();
    return fichier;
  } catch (e) {
    moteurErreur.set(String(e));
    return null;
  } finally {
    sortieNatifBusy.set(false);
  }
}
