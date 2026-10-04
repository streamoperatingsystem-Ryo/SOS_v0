import { mount } from "svelte";
// Système de thème 3 couches (cf. IDEE/IDEA02.md) : tokens (primitives) →
// rôles (contrat + recettes dérivées) → thème actif (glace). Un futur thème
// = un nouveau fichier theme-*.css importé ici à la place de glace.css.
import "./theme/tokens.css";
import "./theme/roles.css";
import "./theme/glace.css";
import "./app.css";
import App from "./App.svelte";

// Neutralise le menu contextuel natif de la webview (Windows/Linux/Mac)
// partout dans le dashboard. N'empêche pas les menus maison .ctx-menu :
// preventDefault supprime l'action par défaut, pas les handlers locaux
// (onCanvasContextMenu / onPastilleContext continuent de s'exécuter).
window.addEventListener("contextmenu", (e) => e.preventDefault());

const app = mount(App, {
  target: document.getElementById("app")!,
});

export default app;
