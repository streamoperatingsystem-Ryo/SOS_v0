// Store de la commande chat « !cadres <couleur> » côté dashboard.
// Détection + cooldown global 60 s + application de la couleur : tout est
// côté Rust (cadres_cmd.rs) — override ÉPHÉMÈRE sur ScenesState.cadres_chat
// injecté dans les snapshots WS :4321, JAMAIS persisté (la couleur du
// streamer prime au redémarrage ; la scène n'est pas mutée). Le dashboard
// n'a donc rien à appliquer : ce fichier ne gère que la config.
// Config persistée : cadres_chat.json.
import { writable, get } from "svelte/store";
import { listen } from "@tauri-apps/api/event";
import {
  tauri,
  type CadreConfig,
  type CadresChatConfig,
  type PaletteCouleur,
} from "../tauri";
import { t } from "./langue";

export const cadresChatStore = writable<CadresChatConfig>({
  actif: true,
  commande: "cadres",
  reponses_chat: true,
});

/// Palette fermée de couleurs nommées (servie par le Rust — affichée dans
/// l'onglet Cadres pour que le streamer connaisse les noms valides).
export const cadresChatPalette = writable<PaletteCouleur[]>([]);

/// Charge la config + la palette depuis Rust (palette en cache — figée).
export async function chargerCadresChat(): Promise<void> {
  try {
    cadresChatStore.set(await tauri.cadresChatEtat());
    if (get(cadresChatPalette).length === 0) {
      cadresChatPalette.set(await tauri.cadresChatPalette());
    }
  } catch (e) {
    console.error("chargerCadresChat:", e);
  }
}

/// Sauvegarde la config (merge partiel). Retourne false en cas de refus
/// Rust (nom vide / collision avec une commande chat, musique ou fight) —
/// l'appelant restaure alors le champ édité.
export async function sauverCadresChat(
  patch: Partial<CadresChatConfig>
): Promise<boolean> {
  const cfg = { ...get(cadresChatStore), ...patch };
  try {
    cadresChatStore.set(await tauri.cadresChatSetConfig(cfg));
    return true;
  } catch (e) {
    alert(get(t)("interactions.cadres.sauveKo", { erreur: String(e) }));
    return false;
  }
}

/// Signature couleur de « !cadres <couleur> » — miroir EXACT de
/// signature_couleurs dans cadres_cmd.rs :
/// - base (couleur) : NOIR #000000 — le « trait principal atténué » du
///   rendu par défaut (CADRE_WIDGET_DEFAUT : #000000 + #8b5cf6). Préserve
///   les nuances d'épaisseur (halo + bande invisibles, seuls les liserés
///   ressortent) et laisse les encoches des biseaux visibles ; une base
///   teintée remplirait toute la largeur du trait.
/// - vive (couleurFin) : la couleur choisie, brute — liserés + ornements.
/// Utilisée par les boutons test de la modale Interactions.
export function signatureCadresChat(
  hex: string
): Pick<CadreConfig, "couleur" | "couleurFin"> {
  return { couleur: "#000000", couleurFin: hex.toLowerCase() };
}

let listenersBound = false;

/// Listener Tauri "cadres:update" (config poussée à chaque save).
/// Idempotent — à appeler au mount du dashboard. La couleur viewer n'a
/// volontairement pas de listener ici : elle est appliquée par le Rust
/// via l'override de snapshot, sans toucher à la scène.
export async function initCadresChat(): Promise<void> {
  if (listenersBound) return;
  listenersBound = true;

  await listen<CadresChatConfig>("cadres:update", (e) => {
    cadresChatStore.set(e.payload);
  });
}
