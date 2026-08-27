// Store OBS côté dashboard. État de connexion + champs formulaire + fonction connect.
// La commande Rust obs_connect est one-shot : connecte, configure scène/source,
// refresh la source, se déconnecte. Le statut "connected" signifie "configuration réussie".
import { writable } from "svelte/store";
import { tauri } from "../tauri";

export type ObsStatus = "idle" | "connecting" | "connected" | "error";

export const obsStatus = writable<ObsStatus>("idle");
export const obsError = writable<string | null>(null);

// Champs formulaire partagés (défauts 127.0.0.1 / 4455).
// Accessibles depuis App.svelte pour l'auto-connect au server_ready.
export const obsHost = writable("127.0.0.1");
export const obsPort = writable("4455");
export const obsPassword = writable("");

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
    obsError.set(String(e));
  }
}
