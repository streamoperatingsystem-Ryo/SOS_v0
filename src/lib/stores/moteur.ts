// Registre du moteur de diffusion (lot 1.5) — mode OBS ou natif.
//
// L'état vient du backend (`moteur_etat`), jamais d'une supposition : les
// capacités affichées sont celles du moteur réellement actif. Les deux
// moteurs sont branchés depuis le lot 1.4 (natif = capture/composition/
// encodage sans OBS, audio loopback + micro muxé).
import { writable, derived, get } from "svelte/store";
import {
  tauri,
  type ApercuNatifCfg,
  type SortieNatifCfg,
  type StreamCfgPersistee,
  type StreamDemarrerCfg,
} from "../tauri";
import { save } from "@tauri-apps/plugin-dialog";
import { appDataDir } from "@tauri-apps/api/path";
import { obsHost, obsPort, obsPassword, obsStatus } from "./obs";
import { sceneStore, syncCapturesOBS } from "./scene";
import { sortieNatifModalOpen } from "./ui";
import { t } from "./langue";

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

/// « schéma://autorité » = flux réseau (rtmp/rtmps/srt) ; tout le reste =
/// chemin de fichier. La session ne porte pas de kind : la destination
/// annoncée par `moteur_sortie_etat` est le seul discriminateur honnête.
export function sortieEstFlux(chemin: string | null): boolean {
  return !!chemin && /^[a-zA-Z][a-zA-Z0-9+.-]*:\/\//.test(chemin);
}

/// Sortie native en cours, typée : flux réseau actif / fichier actif.
/// Alimente les pastilles stream/record du dashboard ET de la barre de
/// raccourcis — jamais croisées (un flux n'allume pas le bouton record).
export const sortieNatifFlux = derived(
  sortieNatif,
  (s) => s.actif && sortieEstFlux(s.chemin),
);
export const sortieNatifFichierActif = derived(
  sortieNatif,
  (s) => s.actif && !sortieEstFlux(s.chemin),
);

/// Réglages de sortie natifs (moteur.json, bloc `sortie`) — consommés au
/// démarrage d'un enregistrement ET d'un stream. Défauts produit tant que
/// `chargerSortieCfg` n'a pas répondu.
export const sortieCfg = writable<SortieNatifCfg>({
  codec: "h264",
  conteneur: "mp4",
  resolution: null,
  debit_kbps: 6000,
  fps: 60,
  preset: "equilibre",
  audio_loopback: true,
  audio_micro: true,
  gain_loopback: 1000,
  gain_micro: 1000,
});

/// Dernière config stream persistée dans le keyring — pré-remplissage du
/// bloc Stream de la modale (null = jamais streamé).
export const streamCfgPersistee = writable<StreamCfgPersistee | null>(null);

/// Relit les réglages persistés (moteur.json). Non-fatal : les défauts du
/// store restent affichés si la lecture échoue.
export async function chargerSortieCfg(): Promise<void> {
  try {
    sortieCfg.set(await tauri.moteurSortieConfig());
  } catch (e) {
    moteurErreur.set(String(e));
  }
}

/// Persiste les réglages de sortie (moteur.json). Le backend refuse une
/// config invalide — l'erreur remonte à l'appelant pour l'afficher.
export async function sauverSortieCfg(cfg: SortieNatifCfg): Promise<void> {
  await tauri.moteurSortieConfigSauver(cfg);
  sortieCfg.set(cfg);
}

/// Réglages du projecteur natif (moteur.json, bloc `apercu`) — qualité et
/// cadence du flux réduit. Défauts produit tant que `chargerApercuCfg`
/// n'a pas répondu.
export const apercuCfg = writable<ApercuNatifCfg>({ largeur: 480, fps: 10 });

/// Relit les réglages du projecteur (non-fatal : défauts conservés).
export async function chargerApercuCfg(): Promise<void> {
  try {
    apercuCfg.set(await tauri.moteurApercuConfig());
  } catch (e) {
    moteurErreur.set(String(e));
  }
}

/// Persiste les réglages du projecteur. Le backend relance une session
/// aperçu en cours — application quasi immédiate hors enregistrement.
export async function sauverApercuCfg(cfg: ApercuNatifCfg): Promise<void> {
  await tauri.moteurApercuConfigSauver(cfg);
  apercuCfg.set(cfg);
}

/// Relit la config stream du keyring (pré-remplissage). Non-fatal.
export async function chargerStreamConfig(): Promise<void> {
  try {
    streamCfgPersistee.set(await tauri.moteurStreamConfig());
  } catch {
    // Coffre vide ou illisible : le formulaire garde ses défauts.
  }
}

/// Démarre le flux natif (RTMP/SRT) vers `p.url` — même session et même
/// arrêt qu'un enregistrement (`arreterEnregistrementNatif`). Retourne la
/// destination publique, ou false sur erreur (moteurErreur rempli).
export async function demarrerStreamNatif(p: StreamDemarrerCfg): Promise<boolean> {
  moteurErreur.set(null);
  sortieNatifBusy.set(true);
  try {
    const cible = await tauri.moteurStreamDemarrer(p);
    sortieNatifFichier.set(cible);
    await rafraichirSortieNatif();
    return true;
  } catch (e) {
    moteurErreur.set(String(e));
    return false;
  } finally {
    sortieNatifBusy.set(false);
  }
}

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

/// Libère le capteur caméra d'OBS avant une bascule vers le Rust : la source
/// dshow tient le périphérique en exclusif. `camera_sync(visible=false)`
/// désactive les items (jamais `RemoveInput`) — la source et ses réglages
/// restent, `syncCapturesOBS` les réactive au retour en mode OBS.
/// Non fatal : OBS offline ou aucun widget caméra → no-op.
async function libererCameraObs(): Promise<void> {
  if (get(obsStatus) !== "connected") return;
  const cam = get(sceneStore).widgets.find((w) => w.type === "camera");
  if (!cam) return;
  try {
    await tauri.cameraSync(
      get(obsHost),
      parseInt(get(obsPort), 10) || 4455,
      get(obsPassword),
      null,
      cam.x,
      cam.y,
      cam.largeur,
      cam.hauteur,
      cam.cameraFondVert === true,
      false,
    );
  } catch {
    // OBS offline / scène SOS absente : l'ouverture native échouera
    // proprement (source omise + dégradation), jamais un crash.
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
    // Caméra : exclusivité dshow (OBS) vs Media Foundation (Rust). La
    // libération n'a lieu qu'APRÈS une bascule acceptée (un refus ne doit
    // pas couper la caméra OBS).
    if (cible === "natif") {
      await libererCameraObs();
    } else {
      // Retour OBS : réactive caméra + trous de la scène courante.
      await syncCapturesOBS();
    }
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

/// Horodatage local `AAAA-MM-JJ_HH-MM-SS` pour le nom par défaut.
function horodatage(): string {
  const d = new Date();
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}_${p(d.getHours())}-${p(d.getMinutes())}-${p(d.getSeconds())}`;
}

/// Dialogue « Enregistrer sous ». Défaut dans `enregistrements/` du dossier
/// de données de l'app (tout local) ; `null` si l'utilisateur annule.
/// Extension et filtre suivent le conteneur persisté (bloc `sortie` de
/// moteur.json) — relecture non fatale, les défauts restent mp4.
async function choisirFichierSortie(): Promise<string | null> {
  await chargerSortieCfg();
  const conteneur = get(sortieCfg).conteneur === "mkv" ? "mkv" : "mp4";
  const nom = `StreamOS-${horodatage()}.${conteneur}`;
  let defaut = nom;
  try {
    defaut = `${await appDataDir()}/enregistrements/${nom}`;
  } catch {
    // Repli : le dialogue s'ouvre sur son dernier dossier connu.
  }
  try {
    return await save({
      title: get(t)("moteur.enregistrerTitre"),
      defaultPath: defaut,
      filters: [{ name: conteneur === "mkv" ? "Matroska" : "MP4", extensions: [conteneur] }],
    });
  } catch (e) {
    moteurErreur.set(String(e));
    return null;
  }
}

/// Toggle « Enregistrer » en mode natif — partagé par les 3 lanceurs
/// (carte sidebar, topbar, barre de raccourcis). Une seule sortie à la
/// fois : un flux actif affiche un message au lieu d'être arrêté
/// implicitement (jamais couper un live sans accord).
export async function toggleEnregistrementNatif(): Promise<void> {
  const s = get(sortieNatif);
  if (s.actif) {
    if (sortieEstFlux(s.chemin)) {
      moteurErreur.set(get(t)("moteur.sortieOccupeeStream"));
      return;
    }
    await arreterEnregistrementNatif();
    return;
  }
  const chemin = await choisirFichierSortie();
  if (chemin) await demarrerEnregistrementNatif(chemin);
}

/// Toggle « Stream » en mode natif : un flux actif est arrêté (même
/// session qu'un enregistrement) ; sinon le démarrage reprend la config
/// persistée du keyring. Jamais configuré → la modale « Sortie » s'ouvre
/// pour saisir protocole + URL + clé au lieu de partir à l'aveugle.
export async function toggleStreamNatif(): Promise<void> {
  const s = get(sortieNatif);
  if (s.actif) {
    if (!sortieEstFlux(s.chemin)) {
      moteurErreur.set(get(t)("moteur.sortieOccupeeEnregistrement"));
      return;
    }
    await arreterEnregistrementNatif();
    return;
  }
  await chargerStreamConfig();
  const cfg = get(streamCfgPersistee);
  if (!cfg) {
    sortieNatifModalOpen.set(true);
    return;
  }
  await demarrerStreamNatif({
    protocole: cfg.protocole,
    url: cfg.url,
    cle: cfg.cle ?? undefined,
    codec: cfg.codec,
    debit_kbps: cfg.debit_kbps,
    fps: cfg.fps,
    preset: cfg.preset,
    srt_passphrase: cfg.srt_passphrase ?? undefined,
    srt_latency_ms: cfg.srt_latency_ms ?? undefined,
  });
}
