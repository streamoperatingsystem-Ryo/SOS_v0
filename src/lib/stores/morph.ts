// État UI du mode morphing (sidebar) — lu par Widget.svelte (clic-pour-
// morpher) et Canvas.svelte (clic fond). Les morphs eux-mêmes vivent dans
// la scène (scene.ts) — ici uniquement les paramètres du PROCHAIN morph.
import { writable } from "svelte/store";

/// Mode morphing actif : les clics sur un média (ou le fond) appliquent un
/// morph au point cliqué au lieu du drag/déplacement.
export const morphMode = writable(false);

/// Sens du prochain morph.
export const morphSens = writable<"agrandir" | "retrecir">("agrandir");

/// Intensité en % (10-200) — comme MorphLens.
export const morphIntensite = writable(35);

/// Rayon en fraction de la largeur visible (0.05-0.5).
export const morphRayon = writable(0.15);
