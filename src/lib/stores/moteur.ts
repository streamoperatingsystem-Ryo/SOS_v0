// Registre du moteur de diffusion (lot 1.5) — mode OBS ou natif.
//
// L'état vient du backend (`moteur_etat`), jamais d'une supposition : les
// capacités affichées sont celles du moteur réellement actif. Le natif reste
// indisponible tant que le lot suivant ne l'a pas branché (`natif_disponible`).
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

/// Défaut honnête : OBS, natif non disponible, aucune capacité inventée.
export const moteurEtat = writable<EtatMoteur>({
  mode: "obs",
  natif_disponible: false,
  obs_disponible: true,
  capacites: [],
});

export const moteurErreur = writable<string | null>(null);

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
/// refuse alors par défaut (jamais de coupure implicite).
export async function basculerMoteur(cible: ModeMoteur, liveActif: boolean): Promise<boolean> {
  moteurErreur.set(null);
  try {
    const mode = await tauri.moteurBasculer(cible, liveActif);
    moteurEtat.update((e) => ({ ...e, mode: mode === "natif" ? "natif" : "obs" }));
    return true;
  } catch (e) {
    moteurErreur.set(String(e));
    return false;
  }
}
