// Store OBS côté dashboard. État de connexion + champs formulaire + fonction connect.
// La commande Rust obs_connect est one-shot : connecte, configure scène/source,
// refresh la source, se déconnecte. Le statut "connected" signifie "configuration réussie".
import { writable, type Writable } from "svelte/store";
import { tauri } from "../tauri";
import { openSectionExplicit } from "./ui";

export type ObsStatus = "idle" | "connecting" | "connected" | "error";

export const obsStatus = writable<ObsStatus>("idle");
export const obsError = writable<string | null>(null);

// Champs formulaire partagés (défauts 127.0.0.1 / 4455).
// Persistés en localStorage (secret local machine-only, pas un token cloud)
// → survivent aux reloads webview (HMR Vite) et aux redémarrages de l'app.
// Sans ça, le mot de passe tapé est perdu à chaque reload → auth OBS 4009.
const LS_HOST = "obsHost";
const LS_PORT = "obsPort";
const LS_PASSWORD = "obsPassword";

function lireLS(cle: string, defaut: string): string {
  try {
    return localStorage.getItem(cle) ?? defaut;
  } catch {
    return defaut; // localStorage indisponible (webview restreinte)
  }
}

export const obsHost = writable(lireLS(LS_HOST, "127.0.0.1"));
export const obsPort = writable(lireLS(LS_PORT, "4455"));
export const obsPassword = writable(lireLS(LS_PASSWORD, ""));

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

/// Traduit une erreur brute (invoke) en message clair et actionnable.
/// Les erreurs techniques restent dans la console (les appelants loggent).
function messageClair(e: unknown): string {
  const s = String(e);
  if (s.includes("authentification échouée")) {
    return "Mot de passe OBS manquant ou invalide. Dans OBS : Outils → WebSocket Server Settings → Show Connect Info, copiez le mot de passe puis collez-le dans le champ ci-dessus.";
  }
  if (s.includes("10061") || s.includes("refus")) {
    return "OBS est injoignable. Vérifiez qu'OBS est ouvert et que le serveur WebSocket est activé (Outils → WebSocket Server Settings).";
  }
  return s;
}

/// Connecte à OBS WebSocket (host:port, password). La commande Rust s'assure
/// que la scène "SOS" + source navigateur "SOS-Diffusion" (1920×1080, URL :4321)
/// existent — idempotent, sans doublon — puis refresh la source.
export async function obsConnect(
  host: string,
  port: string,
  password: string,
): Promise<void> {
  const portNum = parseInt(port, 10);
  if (isNaN(portNum) || portNum < 1 || portNum > 65535) {
    obsStatus.set("error");
    obsError.set("Port invalide");
    return;
  }

  obsStatus.set("connecting");
  obsError.set(null);

  try {
    await tauri.obsConnect(host, portNum, password);
    obsStatus.set("connected");
  } catch (e) {
    obsStatus.set("error");
    const msg = messageClair(e);
    obsError.set(msg);
    // Échec d'auth → ouvrir la section OBS : le champ mot de passe et le
    // message correctif sont immédiatement visibles (sinon l'erreur reste
    // cachée dans une section fermée).
    if (msg.startsWith("Mot de passe OBS")) {
      openSectionExplicit("obs");
    }
    // Propager l'échec : les appelants (bootObs, onConnect) skip les appels
    // OBS suivants (refreshDiffusion, sceneSyncCaptures) quand OBS offline.
    throw e;
  }
}
