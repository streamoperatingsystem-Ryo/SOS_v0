<script lang="ts" module>
  // Un bouton du badge de survol : icône + libellé (title/aria) + action.
  // `disabled` grise le bouton (extrémités de la bande z — boutons −/+).
  export type BoutonSurvol = {
    icone: string;
    titre: string;
    onclick: () => void;
    disabled?: boolean;
  };
</script>

<script lang="ts">
  // Badge de survol UNIFIÉ — widgets et modules HUD (2026-10-30).
  // Avant : la recette .wbadge/.z-controls/.z-btn de Widget.svelte était
  // copiée-renommée .mod-badge/.mod-eye/.mod-eye-btn dans ModuleHud.svelte
  // (~90 lignes CSS + la logique badgePos dupliquées → dérive). Maintenant :
  // UN SEUL composant + UNE recette CSS globale dans app.css — même pattern
  // que CtxMenu.svelte + .ctx-* (aucun <style> ici, tout vit dans app.css).
  //
  // Structure : racine = ancre .wbadge FRÈRE de .widget/.module — PAS un
  // descendant : enfant du widget, le badge serait avalé par son stacking
  // context (z-index:{w.z}) et recouvert par tout voisin à z supérieur et
  // par les modules (8000). L'ancre épouse le rect passé en props
  // (z 9400 : au-dessus des widgets ≤5000, des modules 8000 et des aides
  // 9000 ; sous les poignées .sel-overlay 9500 qui gardent la priorité).
  // pointer-events:none : la zone invisible ne mange aucun clic.
  //
  // Affichage : CSS pur dans app.css — :hover du parent (.widget/.module)
  // + frère adjacent .wbadge, OU .wbadge:hover (le badge déborde du rect :
  // le survoler seul le garde affiché) ; masqué pendant .dragging/.actif.
  // ⚠️ ADJACENCE OBLIGATOIRE : ce composant doit être placé immédiatement
  // après .widget/.module dans le markup parent (sélecteur +).
  //
  // visible : épingle le badge en permanence — un badge qui signale un
  // état actif reste lisible sans survol (caméra masquée / module masqué ;
  // règle « 3 surfaces », docs/features/ctx-menus.md).
  //
  // data-widget-id / data-module-id : portés par l'ancre pour le clic
  // droit (closest('[data-*-id]'), Canvas.svelte) — le menu contextuel
  // vise juste aussi sur le badge.
  //
  // pointerdown/dblclick stoppés : un clic ne doit ni désélectionner, ni
  // démarrer un drag, ni déposer un morph, ni ouvrir une modale.
  import { sceneStore } from "../stores/scene";

  let {
    x,
    y,
    largeur,
    hauteur,
    scale,
    boutons,
    visible = false,
    widgetId,
    moduleId,
  }: {
    x: number;
    y: number;
    largeur: number;
    hauteur: number;
    scale: number;
    boutons: BoutonSurvol[];
    visible?: boolean;
    widgetId?: string;
    moduleId?: string;
  } = $props();

  // ===== Placement adaptatif du badge =====
  // Le badge est contre-scalé (taille écran constante) → l'espace requis
  // en unités canvas = px écran / scale. Ordre : haut → bas → droite →
  // gauche → dedans (overlay interne — dernier recours, toujours visible).
  // La carte peut être à moitié sortie du canvas : chaque bord est testé.
  // Réactif : move/resize/zoom recalculent la position (x/y/largeur/
  // hauteur = valeurs LIVE du parent — store pour les widgets, état local
  // lx/ly pendant le drag pour les modules).
  let badgePos = $derived.by(() => {
    const cw = $sceneStore.canvasW ?? 1920;
    const ch = $sceneStore.canvasH ?? 1080;
    const s = scale > 0 ? scale : 1;
    const nb = Math.max(1, boutons.length); // œil seul (cam/module) vs −/+ (widgets)
    const ligH = 36 / s;                    // ligne : 1 bouton 30 px + écart 6 px
    const colH = (nb * 36 + 6) / s;         // colonne : N boutons + gaps + écart
    if (y >= ligH) return "haut";
    if (ch - y - hauteur >= ligH) return "bas";
    if (cw - x - largeur >= ligH && ch - y >= colH) return "droite";
    if (x >= ligH && ch - y >= colH) return "gauche";
    return "dedans";
  });
</script>

<div
  class="wbadge"
  data-widget-id={widgetId}
  data-module-id={moduleId}
  style="left:{x}px; top:{y}px; width:{largeur}px; height:{hauteur}px;"
  onpointerdown={(e) => e.stopPropagation()}
  ondblclick={(e) => e.stopPropagation()}
  role="presentation"
>
  <div
    class="z-controls {badgePos}"
    class:visible
    style="transform: scale({scale > 0 ? 1 / scale : 1});"
  >
    {#each boutons as b}
      <button
        class="z-btn"
        title={b.titre}
        aria-label={b.titre}
        disabled={b.disabled === true}
        onclick={b.onclick}
      >{b.icone}</button>
    {/each}
  </div>
</div>
