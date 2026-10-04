// Store alertes côté dashboard. Config par type (persistée Rust) uniquement.
// Tous les événements arrivent côté Rust : USERNOTICE/bits via l'IRC
// (twitch_chat.rs) et follows via EventSub WebSocket temps réel
// (twitch_eventsub.rs) → alertes.rs. Le dashboard ne gère QUE la config.
import { writable, get } from "svelte/store";
import { t } from "./langue";
import {
  tauri,
  type AlertesConfig,
  type AlertesOverlayConfig,
  type AlerteTypeConfig,
} from "../tauri";

export const TYPES_ALERTE = [
  "follow",
  "raid",
  "sub",
  "resub",
  "subgift",
  "bits",
] as const;
export type TypeAlerte = (typeof TYPES_ALERTE)[number];

export const alertesConfig = writable<AlertesConfig>({});

/// Config par défaut d'un type (miroir de config_type_defaut côté Rust).
/// Le template par défaut suit la langue UI via les clés i18n
/// `interactions.alertes.defaut.<type>` (fr.json = source de vérité).
export function configDefautType(type: TypeAlerte): AlerteTypeConfig {
  return {
    actif: true,
    duree_ms: 5000,
    texte_template: get(t)(`interactions.alertes.defaut.${type}`),
    taille_px: 48,
    cooldown_viewer_s: 0,
    cooldown_global_s: 0,
    son: undefined,
    media: undefined,
    media_kind: undefined,
  };
}

/// Config effective d'un type (champ absent → défaut).
export function configType(
  cfg: AlertesConfig,
  type: TypeAlerte
): AlerteTypeConfig {
  return cfg[type] ?? configDefautType(type);
}

/// Charge la config depuis Rust. À appeler une fois au boot (App.svelte).
export async function chargerAlertes(): Promise<void> {
  try {
    alertesConfig.set(await tauri.alertesEtat());
  } catch (e) {
    console.error("chargerAlertes:", e);
  }
}

/// Met à jour la config d'un type (store local + persiste via Rust).
export async function sauverConfigType(
  type: TypeAlerte,
  cfg: AlerteTypeConfig
): Promise<void> {
  alertesConfig.update((c) => ({ ...c, [type]: cfg }));
  try {
    await tauri.alertesSetConfig(type, cfg);
  } catch (e) {
    console.error("sauverConfigType:", e);
  }
}

/// Overlay effectif (absent de la config → défaut haut-centré, miroir Rust).
export function overlayEffectif(cfg: AlertesConfig): AlertesOverlayConfig {
  return cfg.overlay ?? { x: 660, y: 60, largeur: 600, hauteur: 120 };
}

/// Remplace la position/taille de l'overlay (store local + persiste via Rust).
export async function sauverOverlay(ov: AlertesOverlayConfig): Promise<void> {
  alertesConfig.update((c) => ({ ...c, overlay: ov }));
  try {
    await tauri.alertesSetOverlayConfig(ov.x, ov.y, ov.largeur, ov.hauteur);
  } catch (e) {
    console.error("sauverOverlay:", e);
  }
}
