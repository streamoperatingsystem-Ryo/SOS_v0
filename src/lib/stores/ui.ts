// Store UI : section d'accordéon ouverte (une seule à la fois) + carte d'édition.
import { writable } from "svelte/store";

export type Section =
  | "widgets"
  | "connexions"
  | "communaute"
  | "obs"
  | "personnalisation"
  | "raccourcis";
// NB : "Interactions chat" n'est PAS une section d'accordéon — c'est un
// lanceur direct qui ouvre interactionModalOpen (modale à onglets).
// NB : "scene" (Arrière-plan de l'application) a été retiré de l'accordéon —
// l'édition du fond se fait maintenant via la carte d'édition contextuelle
// (CarteEdition.svelte, cible "fond").

/// Section d'accordéon ouverte (une seule à la fois).
/// null au boot = TOUTES les sections pliées (dashboard épuré au lancement).
/// Les sections s'ouvrent ensuite : clic sur l'en-tête, ou erreur actionnable
/// (section OBS auto-ouverte par obs.ts).
/// L'édition d'un widget ou du fond se fait via la carte d'édition (mutuellement
/// exclusive avec l'accordéon — quand la carte apparaît, l'accordéon se ferme).
export const openSection = writable<Section | null>(null);

/// Cible de la carte d'édition contextuelle (en bas du panneau latéral).
/// "widget" = un widget est sélectionné (carte rouge), "fond" = le fond de
/// l'application est sélectionné (carte orange), null = carte masquée.
/// NB : un widget input-viewer sélectionné affiche une carte jaune (géré dans
/// CarteEdition.svelte via la classe dérivée carteClasse — la cible reste
/// "widget", seul le rendu CSS change).
/// Mutuellement exclusive avec openSection : quand la carte apparaît,
/// l'accordéon se ferme, et inversement.
export type CibleEdition = "widget" | "fond" | null;
export const carteEditionCible = writable<CibleEdition>(null);

/// Bascule une section : si déjà ouverte → ferme, sinon → ouvre (et ferme l'autre).
/// Ouvrir une section ferme la carte d'édition (mutuellement exclusifs).
export function toggleSection(section: Section): void {
  openSection.update((cur) => {
    const nouveau = cur === section ? null : section;
    if (nouveau !== null) carteEditionCible.set(null);
    return nouveau;
  });
}

/// Ouvre une section explicitement (sans toggle).
/// Ferme la carte d'édition (mutuellement exclusifs).
export function openSectionExplicit(section: Section): void {
  carteEditionCible.set(null);
  openSection.set(section);
}

/// Active la carte d'édition contextuelle pour la cible donnée.
/// Ferme l'accordéon (mutuellement exclusifs).
export function setCarteEdition(cible: Exclude<CibleEdition, null>): void {
  openSection.set(null);
  carteEditionCible.set(cible);
}

/// État partagé de confirmation suppression widget (bouton Toolbar + touche Suppr).
/// true = affiche les boutons "Oui/Non" dans la Toolbar.
export const confirmDeleteWidget = writable(false);

/// Modale galerie de cadres : null = fermée, "widget" = galerie cadre widget,
/// "app" = galerie cadre application.
export const cadreModalOpen = writable<"widget" | "app" | null>(null);

/// Modale Interactions chat : true = ouverte (onglets Clip de bienvenue /
/// Bandeau premier message / Alertes).
export const interactionModalOpen = writable(false);

/// Modale Modération & Rôles : true = ouverte (onglets par plateforme).
/// Seul l'onglet Twitch est fonctionnel ; les autres affichent "Bientôt disponible".
export const moderationModalOpen = writable(false);

/// Modale d'édition du titre d'un widget : null = fermée, sinon contient
/// l'id du widget à éditer. Ouverte par double-clic sur le haut/bas d'un
/// widget (Widget.svelte ondblclick).
export const titreModalOpen = writable<string | null>(null);

/// Modale d'édition du titre du fond de l'application : false = fermée,
/// true = ouverte. Ouverte par le bouton "Titre du fond" dans la Toolbar
/// (section Arrière-plan de l'application).
export const titreFondModalOpen = writable(false);

/// Modale de configuration ASL (auto-splitter) : false = fermée, true = ouverte.
/// S'ouvre automatiquement quand l'utilisateur charge un ASL qui expose des
/// settings individuels (120+ pour MGS). Permet de cocher/décocher les splits
/// comme dans LiveSplit, avec auto-mapping depuis les segments LSS.
export const speedrunConfigModalOpen = writable(false);
