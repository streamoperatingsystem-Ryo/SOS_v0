// Store UI : section d'accordéon ouverte (une seule à la fois).
import { writable } from "svelte/store";

export type Section = "scene" | "widgets" | "connexions" | "obs" | "personnalisation";

export const openSection = writable<Section | null>("scene");

/// Bascule une section : si déjà ouverte → ferme, sinon → ouvre (et ferme l'autre).
export function toggleSection(section: Section): void {
  openSection.update((cur) => (cur === section ? null : section));
}

/// Ouvre une section explicitement (sans toggle).
export function openSectionExplicit(section: Section): void {
  openSection.set(section);
}

/// État partagé de confirmation suppression widget (bouton Toolbar + touche Suppr).
/// true = affiche les boutons "Oui/Non" dans la Toolbar.
export const confirmDeleteWidget = writable(false);

/// Modale galerie de cadres : null = fermée, "widget" = galerie cadre widget,
/// "app" = galerie cadre application.
export const cadreModalOpen = writable<"widget" | "app" | null>(null);
