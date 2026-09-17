// Store alertes côté dashboard. Config par type (persistée Rust) + détection
// des follows par diff des snapshots Helix (pas d'event IRC pour les follows —
// même approche que l'ancienne app). Les événements USERNOTICE/bits arrivent
// directement côté Rust (twitch_chat.rs → alertes.rs) : le dashboard ne gère
// QUE la config et la diff des follows.
import { writable } from "svelte/store";
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
export function configDefautType(type: TypeAlerte): AlerteTypeConfig {
  const templates: Record<TypeAlerte, string> = {
    follow: "{pseudo} vient de suivre !",
    raid: "{pseudo} raide avec {nbViewers} viewers !",
    sub: "{pseudo} vient de s'abonner !",
    resub: "{pseudo} resub pour {nbMoisCumul} mois !",
    subgift: "{pseudo} offre un sub à {destinataire} !",
    bits: "{pseudo} a envoyé {nbBits} bits !",
  };
  return {
    actif: true,
    duree_ms: 5000,
    texte_template: templates[type],
    couleur: "#ffffff",
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

// ===== Détection des follows (diff snapshots Helix) =====
// Pas d'event IRC pour les follows : on poll la liste Helix toutes les 60s
// et on déclenche pour les nouveaux (user_id inconnu ET followed_at post-boot
// — élimine les faux positifs quand un follower entre dans la fenêtre paginée).

const POLL_MS = 60_000;
/// Followers connus (user_id). Rempli au premier poll (baseline, pas d'alerte).
const connus = new Set<string>();
/// Date de boot (ISO) — un follow antérieur n'est jamais "nouveau".
const bootIso = new Date().toISOString();
let pollTimer: ReturnType<typeof setInterval> | null = null;
let baselineFait = false;

async function pollFollowers(): Promise<void> {
  try {
    const resp = await tauri.twitchFollowersLight();
    for (const f of resp) {
      if (connus.has(f.user_id)) continue;
      connus.add(f.user_id);
      // Nouveau = connu après la baseline ET follow postérieur au boot.
      // (ISO 8601 UTC : comparaison lexicographique fiable.)
      const nouveau = baselineFait && f.followed_at > bootIso;
      if (nouveau) {
        tauri
          .alerteDeclencher("follow", f.login, f.user_id, 0, 0, 0, "")
          .catch(() => {});
      }
    }
    baselineFait = true;
  } catch {
    // Twitch déconnecté / need_reauth / offline → silencieux, on retentera.
  }
}

/// Démarre le polling des follows (baseline au 1er poll, alertes ensuite).
/// Idempotent. Appelé au boot (App.svelte) — le polling tourne même si Twitch
/// est déconnecté (la commande échoue silencieusement et on retente).
export function demarrerDiffFollows(): void {
  if (pollTimer) return;
  void pollFollowers();
  pollTimer = setInterval(pollFollowers, POLL_MS);
}
