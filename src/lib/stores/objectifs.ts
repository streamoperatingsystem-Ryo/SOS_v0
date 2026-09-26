// objectifs.ts — valeurs live des métriques des widgets "goal".
// Alimenté par les events backend `communaute:update` (viewers.rs) et
// `viewers:update` (viewers.rs + tiktok_chat.rs) — les mêmes données que la
// diffusion reçoit sur :4321. Source unique pour la carte d'édition (bouton
// « Suggestion » de palier) et l'aperçu dashboard du widget goal.
import { writable } from "svelte/store";
import { listen } from "@tauri-apps/api/event";
import type { GoalMetrique, GoalPlateforme } from "../tauri";

export type ValeursPlateforme = Partial<Record<GoalMetrique, number>>;

/// Dernière valeur connue par plateforme → métrique. Métrique absente =
/// jamais reçue ou plateforme hors-ligne (afficher « N/A »).
export const valeursGoals = writable<
  Partial<Record<GoalPlateforme, ValeursPlateforme>>
>({});

function setValeur(plat: GoalPlateforme, met: GoalMetrique, n: number | null): void {
  valeursGoals.update((m) => {
    const p = { ...(m[plat] ?? {}) };
    if (n === null || !Number.isFinite(n)) delete p[met];
    else p[met] = n;
    return { ...m, [plat]: p };
  });
}

let inited = false;

/// Enregistre les listeners Tauri (idempotent). À appeler au mount du
/// dashboard, après initChat (même pattern que initBandeau).
export async function initObjectifs(): Promise<void> {
  if (inited) return;
  inited = true;
  await listen<{ plateforme: string; count: number | null }>(
    "viewers:update",
    (e) => {
      setValeur(e.payload.plateforme as GoalPlateforme, "viewers", e.payload.count);
    },
  );
  await listen<Record<string, unknown>>("communaute:update", (e) => {
    const plat = e.payload.plateforme as GoalPlateforme;
    for (const met of ["followers", "subs", "sub_points", "abonnes"] as const) {
      const v = e.payload[met];
      if (typeof v === "number") setValeur(plat, met, v);
    }
  });
}
