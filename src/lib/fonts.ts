// Polices Google Font embarquées (woff2, subset latin) pour les titres de
// widgets. Servies par le serveur :4321 sur /fonts/{name}. Le dashboard les
// charge via @font-face (cross-origin :4321 — CORS permissif côté Rust).
// La diffusion (même origine :4321) les charge via ses propres @font-face
// dans diffusion.html.

export interface PoliceTitre {
  /// Identifiant = nom du fichier woff2 (sans extension) = nom de famille CSS.
  id: string;
  /// Libellé affiché dans le sélecteur.
  nom: string;
}

/// Liste des 20 polices disponibles pour les titres de widgets.
/// L'ordre = ordre d'affichage dans le sélecteur de la modale Titre.
export const POLICES_TITRE: PoliceTitre[] = [
  { id: "BebasNeue", nom: "Bebas Neue" },
  { id: "Anton", nom: "Anton" },
  { id: "Oswald", nom: "Oswald" },
  { id: "Pacifico", nom: "Pacifico" },
  { id: "PermanentMarker", nom: "Permanent Marker" },
  { id: "Bungee", nom: "Bungee" },
  { id: "RussoOne", nom: "Russo One" },
  { id: "PressStart2P", nom: "Press Start 2P" },
  { id: "Orbitron", nom: "Orbitron" },
  { id: "Montserrat", nom: "Montserrat" },
  { id: "Righteous", nom: "Righteous" },
  { id: "Audiowide", nom: "Audiowide" },
  { id: "Lobster", nom: "Lobster" },
  { id: "Caveat", nom: "Caveat" },
  { id: "Satisfy", nom: "Satisfy" },
  { id: "Teko", nom: "Teko" },
  { id: "Rajdhani", nom: "Rajdhani" },
  { id: "BlackOpsOne", nom: "Black Ops One" },
  { id: "Creepster", nom: "Creepster" },
  { id: "Fredoka", nom: "Fredoka" },
];

/// Police par défaut pour un nouveau titre.
export const POLICE_TITRE_DEFAUT = "BebasNeue";

/// Taille par défaut (px) pour un nouveau titre.
export const TAILLE_TITRE_DEFAUT = 24;

/// Espacement des lettres par défaut (px).
export const ESPACEMENT_TITRE_DEFAUT = 0;

/// Préfixe URL pour les polices servies par :4321 (dashboard ≠ :4321).
const FONT_BASE = "http://127.0.0.1:4321/fonts/";

let injected = false;

/// Injecte les règles @font-face pour les 20 polices dans le <head> du
/// dashboard. Idempotent (n'injecte qu'une fois). Appelée au montage de
/// l'app (App.svelte onMount).
export function injecterFontsCSS(): void {
  if (injected) return;
  injected = true;
  let css = "";
  for (const p of POLICES_TITRE) {
    css += `@font-face { font-family: '${p.id}'; font-style: normal; font-weight: 400; font-display: swap; src: url(${FONT_BASE}${p.id}.woff2) format('woff2'); }\n`;
  }
  const style = document.createElement("style");
  style.setAttribute("data-sos-fonts", "");
  style.textContent = css;
  document.head.appendChild(style);
}
