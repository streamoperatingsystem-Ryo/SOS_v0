// Store OBS côté dashboard. État de connexion + champs formulaire + fonction connect.
// La commande Rust obs_connect est one-shot : connecte, configure scène/source,
// refresh la source, se déconnecte. Le statut "connected" signifie "configuration réussie".
import { writable, get, type Writable } from "svelte/store";
import { tauri, type ObsMoniteur } from "../tauri";
import { openSectionExplicit } from "./ui";
import { t } from "./langue";

export type ObsStatus = "idle" | "connecting" | "connected" | "error";

export const obsStatus = writable<ObsStatus>("idle");
export const obsError = writable<string | null>(null);

// État des sorties OBS (stream / record). PAS de polling : lu au connect,
// relu à l'ouverture de la modale de confirmation / section OBS / focus
// fenêtre, et mis à jour après chaque set (le retour de obs_set_* donne
// l'état réel relu). Peut être momentanément désynchronisé si l'utilisateur
// agit directement dans OBS — l'action confirmée reste toujours correcte
// (Start/Stop explicite, jamais de toggle aveugle).
export const obsStreaming = writable<boolean>(false);
export const obsRecording = writable<boolean>(false);

// Toggle en vol : une connexion OBS one-shot par appel — grise les boutons
// des lanceurs (Toolbar, topbar, modale de confirmation) pendant le RPC et
// sert de garde anti-réentrance dans les toggles ci-dessous.
export const obsStreamBusy = writable(false);
export const obsRecordBusy = writable(false);

// Champs formulaire partagés (défauts 127.0.0.1 / 4455).
// Persistés en localStorage (secret local machine-only, pas un token cloud)
// → survivent aux reloads webview (HMR Vite) et aux redémarrages de l'app.
// Sans ça, le mot de passe tapé est perdu à chaque reload → auth OBS 4009.
const LS_HOST = "obsHost";
const LS_PORT = "obsPort";
const LS_PASSWORD = "obsPassword";
// Options WebSocket pilotées depuis la modale « Connexion OBS »
// (ObsWebSocketModal) — mêmes défauts que le comportement historique.
const LS_AUTO_CONNECT = "obsAutoConnect";
const LS_FERMER_PROJECTEURS = "obsFermerProjecteurs";

function lireLS(cle: string, defaut: string): string {
  try {
    return localStorage.getItem(cle) ?? defaut;
  } catch {
    return defaut; // localStorage indisponible (webview restreinte)
  }
}

function lireLSBool(cle: string, defaut: boolean): boolean {
  return lireLS(cle, defaut ? "1" : "0") === "1";
}

export const obsHost = writable(lireLS(LS_HOST, "127.0.0.1"));
export const obsPort = writable(lireLS(LS_PORT, "4455"));
export const obsPassword = writable(lireLS(LS_PASSWORD, ""));
/// Connexion automatique à OBS quand le serveur :4321 est prêt (bootObs).
/// false = l'utilisateur se connecte à la main depuis la section OBS.
export const obsAutoConnect = writable(lireLSBool(LS_AUTO_CONNECT, true));
/// Fermer les fenêtres projecteur d'OBS à chaque connexion réussie
/// (OBS les restaure d'une session à l'autre). false = les laisser.
export const obsFermerProjecteurs = writable(lireLSBool(LS_FERMER_PROJECTEURS, true));

/// Persiste un store string en localStorage à chaque changement.
/// subscribe fire immédiatement → réécrit la valeur chargée (idempotent).
function persister(store: Writable<string>, cle: string): void {
  store.subscribe((v) => {
    try {
      localStorage.setItem(cle, v);
    } catch {
      // ignore (quota / mode privé) — la connexion marche quand même en RAM
    }
  });
}
persister(obsHost, LS_HOST);
persister(obsPort, LS_PORT);
persister(obsPassword, LS_PASSWORD);
obsAutoConnect.subscribe((v) => {
  try {
    localStorage.setItem(LS_AUTO_CONNECT, v ? "1" : "0");
  } catch {
    // ignore
  }
});
obsFermerProjecteurs.subscribe((v) => {
  try {
    localStorage.setItem(LS_FERMER_PROJECTEURS, v ? "1" : "0");
  } catch {
    // ignore
  }
});

/// Traduit une erreur brute (invoke) en message clair et actionnable.
/// Les erreurs techniques restent dans la console (les appelants loggent).
function messageClair(e: unknown): string {
  const s = String(e);
  if (s.includes("authentification échouée")) {
    return get(t)("obs.erreurAuth");
  }
  if (s.includes("10061") || s.includes("refus")) {
    return get(t)("obs.erreurInjoignable");
  }
  return s;
}

/// true quand le boot OBS (connect + refresh + sync) a déjà été DÉCLENCHÉ
/// dans cette instance de module. Ce store vit hors du composant : il survit
/// à un remontage du frontend (HMR Vite en dev, reload de la WebView en prod),
/// contrairement au `serverOk` local d'App.svelte qui repasse à false et
/// rejouait tout le boot OBS. Sans ce garde-fou, chaque remontage relance
/// `obsConnect` → `SetCurrentProgramScene("SOS")` côté OBS (constat
/// 2026-10-27 : 10 bascules de scène en 52 s sans intervention).
/// ⚠️ Le timer d'auto-reconnect 20 s d'App.svelte n'est PAS gardé par ce flag :
/// une déconnexion réelle doit continuer à se rattraper toute seule.
export const obsBootDeclenche = writable(false);

/// Connecte à OBS WebSocket (host:port, password). La commande Rust s'assure
/// que la scène "SOS" + source navigateur "SOS-Diffusion" (1920×1080, URL :4321)
/// existent — idempotent, sans doublon — puis refresh la source.
export async function obsConnect(
  host: string,
  port: string,
  password: string,
  /// Origine de l'appel — tracée dans la boîte noire ("boot" / "bouton" /
  /// "raccourci"). Chaque obs_connect bascule la scène OBS sur "SOS" hors
  /// live : ce paramètre dit QUI l'a déclenché.
  origine?: string,
): Promise<void> {
  const portNum = parseInt(port, 10);
  if (isNaN(portNum) || portNum < 1 || portNum > 65535) {
    obsStatus.set("error");
    obsError.set(get(t)("obs.portInvalide"));
    return;
  }

  obsStatus.set("connecting");
  obsError.set(null);

  try {
    await tauri.obsConnect(host, portNum, password, origine);
    obsStatus.set("connected");
    // Lire l'état des sorties dans la foulée — AWAITÉ : un rafraîchissement
    // encore en vol pendant un set pourrait réécrire le store avec une
    // valeur lue AVANT l'action (race → état inversé affiché).
    await rafraichirOutputs();
    // Projecteur : fermé par défaut. OBS restaure ses fenêtres projecteur
    // d'une session à l'autre → on les ferme activement à chaque connexion
    // réussie. Non-fatal + silencieux (pas d'obsError au boot).
    // Option obsFermerProjecteurs (modale Connexion OBS) : OFF = les
    // projecteurs restaurés par OBS sont laissés tels quels.
    if (get(obsFermerProjecteurs)) {
      void tauri
        .obsProjecteurFermer()
        .catch((e) => console.warn("obs projecteur fermer:", e));
    }
  } catch (e) {
    obsStatus.set("error");
    const msg = messageClair(e);
    obsError.set(msg);
    // Échec d'auth → ouvrir la section OBS : le champ mot de passe et le
    // message correctif sont immédiatement visibles (sinon l'erreur reste
    // cachée dans une section fermée).
    if (String(e).includes("authentification échouée")) {
      openSectionExplicit("obs");
    }
    // Propager l'échec : les appelants (bootObs, onConnect) skip les appels
    // OBS suivants (refreshDiffusion, sceneSyncCaptures) quand OBS offline.
    throw e;
  }
}

// ===== Sorties OBS : stream & record =====
// Chaque action = une connexion OBS one-shot (connect → RPC → close), comme
// toutes les commandes OBS du backend.

function obsCreds(): { host: string; port: number; password: string } {
  return {
    host: get(obsHost),
    port: parseInt(get(obsPort), 10) || 4455,
    password: get(obsPassword),
  };
}

/// Lit l'état des deux sorties (une connexion). Non-fatal : en cas d'erreur
/// les stores restent sur leur dernière valeur connue.
/// Garde anti-race : ignoré si un set est en vol — sinon la réponse GET
/// (lue AVANT l'action) pourrait écraser l'état retourné par le set.
export async function rafraichirOutputs(): Promise<void> {
  if (get(obsStreamBusy) || get(obsRecordBusy)) return;
  const { host, port, password } = obsCreds();
  try {
    const s = await tauri.obsOutputsStatus(host, port, password);
    obsStreaming.set(s.stream);
    obsRecording.set(s.record);
  } catch (e) {
    console.warn("obs outputs_status:", e);
  }
}

/// Met le stream dans l'état demandé (Start/StopStream explicite, idempotent
/// — l'action est TOUJOURS celle confirmée dans l'UI, jamais un toggle
/// aveugle qui inverserait un état réel désynchronisé).
/// Le booléen retourné = outputActive relu dans OBS après l'action.
/// Lève l'erreur traduite pour l'affichage (obsError).
export async function setStreamObs(actif: boolean): Promise<void> {
  if (get(obsStreamBusy)) return;
  const { host, port, password } = obsCreds();
  obsStreamBusy.set(true);
  try {
    obsStreaming.set(await tauri.obsSetStream(host, port, password, actif));
    obsError.set(null);
  } catch (e) {
    obsError.set(messageClair(e));
    throw e;
  } finally {
    obsStreamBusy.set(false);
  }
}

/// Met l'enregistrement dans l'état demandé. Même contrat que setStreamObs.
export async function setRecordObs(actif: boolean): Promise<void> {
  if (get(obsRecordBusy)) return;
  const { host, port, password } = obsCreds();
  obsRecordBusy.set(true);
  try {
    obsRecording.set(await tauri.obsSetRecord(host, port, password, actif));
    obsError.set(null);
  } catch (e) {
    obsError.set(messageClair(e));
    throw e;
  } finally {
    obsRecordBusy.set(false);
  }
}

// ===== Projecteur « Prévisualisation » (section Toolbar) =====
// Liste des écrans vus par OBS (index OBS, pas Win32) + ouverture du
// projecteur preview — clic droit « Ouvrir le projecteur » d'OBS depuis le
// dashboard. Connexions one-shot comme toutes les commandes OBS.

/// Moniteurs OBS pour le select (vide si OBS non connecté ou non relu).
export const obsMoniteurs = writable<ObsMoniteur[]>([]);

/// Relit la liste des moniteurs OBS. Non-fatal : [] en cas d'échec
/// (OBS absent / non connecté) — le select affiche juste « Fenêtré ».
export async function chargerMoniteursObs(): Promise<void> {
  const { host, port, password } = obsCreds();
  try {
    obsMoniteurs.set(await tauri.obsProjecteurMoniteurs(host, port, password));
  } catch (e) {
    console.warn("obs moniteurs:", e);
    obsMoniteurs.set([]);
  }
}

/// Ouvre le projecteur OBS sur la prévisualisation (clic droit OBS).
/// monitorIndex : index OBS (obsMoniteurs) ; -1 = projecteur fenêtré.
/// Lève l'erreur traduite pour l'affichage (obsError).
export async function ouvrirProjecteurPreview(monitorIndex: number): Promise<void> {
  const { host, port, password } = obsCreds();
  try {
    await tauri.obsProjecteurPreview(host, port, password, monitorIndex);
    obsError.set(null);
  } catch (e) {
    obsError.set(messageClair(e));
    throw e;
  }
}
