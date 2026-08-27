// Store UI : section d'accordéon ouverte (une seule à la fois).
import { writable } from "svelte/store";

export type Section = "scene" | "widgets" | "obs";

export const openSection = writable<Section | null>("scene");

/// Bascule une section : si déjà ouverte → ferme, sinon → ouvre (et ferme l'autre).
export function toggleSection(section: Section): void {
  openSection.update((cur) => (cur === section ? null : section));
}

/// Ouvre une section explicitement (sans toggle).
export function openSectionExplicit(section: Section): void {
  openSection.set(section);
}
