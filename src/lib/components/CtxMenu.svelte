<script lang="ts">
  // Wrapper partagé des menus contextuels clic droit — recette .ctx-menu
  // (app.css). Factorise le boilerplate dupliqué des 3 menus (pastille de
  // SceneBar, fond et widget de Canvas) :
  //   - <div class="ctx-menu"> fixed aux coords souris (x/y), role="menu",
  //     stopPropagation click/keydown, preventDefault contextmenu.
  //   - Fermeture : clic extérieur + Escape (svelte:window interne — le clic
  //     droit n'émet pas click, ouvrir le menu ne le ferme pas).
  //   - Clamping viewport : le menu reste dans l'écran (marge 8 px),
  //     re-mesuré via ResizeObserver quand son contenu grandit (ex. le bloc
  //     de confirmation inline) — avant, un menu ouvert en bas/droite de
  //     l'écran débordait hors champ.
  // Usage : <CtxMenu x={menuX} y={menuY} onclose={fermerMenu}>items…</CtxMenu>
  // (les dropdowns ancrés sous un bouton n'utilisent PAS ce composant :
  // ils posent .ctx-menu en position:absolute, cf. .langues-dropdown).
  import type { Snippet } from "svelte";

  let {
    x,
    y,
    onclose,
    children,
  }: {
    x: number;
    y: number;
    onclose: () => void;
    children: Snippet;
  } = $props();

  const MARGE = 8; // px

  let menuEl: HTMLDivElement | undefined = $state(undefined);
  // undefined = pas encore mesuré : le style retombe sur x/y (position
  // souris dès le premier rendu — pas de flash en 0,0), puis le clamp
  // corrige si le menu déborde de l'écran.
  let posX: number | undefined = $state(undefined);
  let posY: number | undefined = $state(undefined);

  // Position clampée dans le viewport : calculée à l'ouverture puis à
  // chaque changement de taille du panneau (confirmation inline, liste
  // qui grandit). window.innerWidth/Height lus à chaque mesure (le
  // redimensionnement fenêtre est couvert par le ResizeObserver seulement
  // si le menu change — un resize fenêtre ne re-clampe pas, cas marginal).
  $effect(() => {
    void x;
    void y;
    if (!menuEl) return;
    const el = menuEl;
    function clamper() {
      const r = el.getBoundingClientRect();
      posX = Math.max(MARGE, Math.min(x, window.innerWidth - r.width - MARGE));
      posY = Math.max(MARGE, Math.min(y, window.innerHeight - r.height - MARGE));
    }
    clamper();
    const ro = new ResizeObserver(clamper);
    ro.observe(el);
    return () => ro.disconnect();
  });

  function onWindowKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") onclose();
  }
</script>

<!-- Clic n'importe où → ferme le menu (le menu stoppe sa propre
     propagation). Escape idem. -->
<svelte:window onclick={onclose} onkeydown={onWindowKeydown} />

<div
  class="ctx-menu"
  bind:this={menuEl}
  style="left:{posX ?? x}px; top:{posY ?? y}px;"
  onclick={(e) => e.stopPropagation()}
  oncontextmenu={(e) => e.preventDefault()}
  onkeydown={(e) => e.stopPropagation()}
  role="menu"
  tabindex="-1"
>
  {@render children()}
</div>
