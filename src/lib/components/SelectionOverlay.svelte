<script lang="ts">
  // Overlay de sélection : poignées resize du widget sélectionné (4 coins +
  // 4 bords). Posé en FRERE des .widget dans Canvas.svelte, au-dessus de tout
  // (z-index au-dessus des widgets ~<5000 et des modules HUD 9000) → les
  // poignées restent visibles/cliquables même quand un widget voisin à z
  // supérieur chevauche le widget sélectionné (avant : enfants du widget,
  // elles étaient avalées par le stacking context).
  // Hit-box ~26px ÉCRAN constante quel que soit le zoom du canvas
  // (--hs = 1/scale, même contre-scale que les z-controls) ; le carré visuel
  // ambre 12px est centré dedans via ::after.
  import {
    resizeWidgetLocal,
    commitScene,
    selectWidget,
    buildSnapTargets,
    computeSnap,
    sceneStore,
    type SnapTarget,
  } from "../stores/scene";
  import { afficherGuidesDrag, masquerGuides } from "../stores/guides";
  import { setCarteEdition } from "../stores/ui";
  import { get } from "svelte/store";
  import type { Widget } from "../tauri";

  let { w, scale }: { w: Widget; scale: number } = $props();

  // Mini widget — jamais en dessous (même borne que Widget.svelte).
  const MIN = 80;
  // Poignées resize : 4 coins + 4 bords.
  const HANDLES = ["nw", "n", "ne", "e", "se", "s", "sw", "w"] as const;

  let overlayEl: HTMLDivElement;

  let resizing = $state(false);
  let handle = "";
  let startX = 0;
  let startY = 0;
  let startWX = 0;
  let startWY = 0;
  let startW = 0;
  let startH = 0;

  // Grille magnétique : cibles figées au pointerdown (snapshot, pas de get() par frame).
  // Bypass si Alt enfoncé (free-resize sans aimantation).
  let snapTargets: { xs: SnapTarget[]; ys: SnapTarget[] } | null = null;

  function onHandleDown(e: PointerEvent, h: string) {
    // Bouton gauche uniquement : le clic droit ouvre le menu contextuel du
    // widget (contextmenu remonte à Canvas).
    if (e.button !== 0) return;
    e.stopPropagation();
    selectWidget(w.id);
    setCarteEdition("widget");
    resizing = true;
    handle = h;
    startX = e.clientX;
    startY = e.clientY;
    startWX = w.x;
    startWY = w.y;
    startW = w.largeur;
    startH = w.hauteur;
    // Snapshot des cibles de snap (figé pendant tout le resize).
    snapTargets = buildSnapTargets(get(sceneStore), w.id);
    // Afficher les guides d'alignement (dashboard uniquement).
    afficherGuidesDrag({ x: w.x, y: w.y, width: w.largeur, height: w.hauteur });
    // Capture sur l'overlay : move/up écoutés sur l'overlay.
    overlayEl.setPointerCapture(e.pointerId);
  }

  function onpointermove(e: PointerEvent) {
    if (!resizing) return;
    const dx = (e.clientX - startX) / scale;
    const dy = (e.clientY - startY) / scale;
    let nw = startW;
    let nh = startH;
    let nx = startWX;
    let ny = startWY;
    // Directions horizontales.
    if (handle.includes("w")) {
      nw = startW - dx;
      if (nw < MIN) {
        nx = startWX + (startW - MIN);
        nw = MIN;
      } else {
        nx = startWX + dx;
      }
    } else if (handle.includes("e")) {
      nw = Math.max(MIN, startW + dx);
    }
    // Directions verticales.
    if (handle.includes("n")) {
      nh = startH - dy;
      if (nh < MIN) {
        ny = startWY + (startH - MIN);
        nh = MIN;
      } else {
        ny = startWY + dy;
      }
    } else if (handle.includes("s")) {
      nh = Math.max(MIN, startH + dy);
    }
    // Grille magnétique sur le resize sauf si Alt enfoncé.
    if (!e.altKey && snapTargets) {
      const snap = computeSnap(nx, ny, nw, nh, snapTargets);
      // Ajuste position selon le snap (les dims restent les mêmes sauf
      // pour les poignées w/n où le snap décale aussi la taille).
      const ddx = snap.x - nx;
      const ddy = snap.y - ny;
      nx = snap.x;
      ny = snap.y;
      if (handle.includes("w")) nw -= ddx;
      if (handle.includes("n")) nh -= ddy;
    }
    resizeWidgetLocal(w.id, nx, ny, nw, nh);
    // Mettre à jour les guides visuels pendant le resize.
    afficherGuidesDrag({ x: nx, y: ny, width: nw, height: nh });
  }

  async function onpointerup(e: PointerEvent) {
    if (!resizing) return;
    resizing = false;
    handle = "";
    snapTargets = null;
    masquerGuides();
    overlayEl.releasePointerCapture(e.pointerId);
    await commitScene();
  }
</script>

<!-- Overlay transparent plaqué sur le rect du widget sélectionné. Lui-même
     pointer-events:none (le drag/sélection du widget reste libre) — seules
     les poignées captent le pointeur. -->
<div
  class="sel-overlay"
  bind:this={overlayEl}
  style="left:{w.x}px; top:{w.y}px; width:{w.largeur}px; height:{w.hauteur}px; --hs:{scale > 0 ? 1 / scale : 1};"
  onpointermove={onpointermove}
  onpointerup={onpointerup}
  role="presentation"
>
  {#each HANDLES as h (h)}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="handle {h}" onpointerdown={(e) => onHandleDown(e, h)}></div>
  {/each}
</div>

<style>
  .sel-overlay {
    position: absolute;
    /* Au-dessus des widgets (z<5000, caméra incluse) et des modules HUD
       (9000) : les poignées ne sont jamais couvertes par un voisin. */
    z-index: 9500;
    pointer-events: none;
  }
  /* Poignées : hit-box 26px ÉCRAN constante (× --hs = contre-scale du zoom
     canvas), centrée pile sur le coin/bord (13px dedans, 13px dehors). Le
     carré visible (12px ambre « action » + liseré sombre) est peint par
     ::after au centre — on vise large, on voit fin. */
  .handle {
    position: absolute;
    width: calc(26px * var(--hs));
    height: calc(26px * var(--hs));
    pointer-events: auto;
    box-sizing: border-box;
  }
  .handle::after {
    content: "";
    position: absolute;
    inset: calc(7px * var(--hs));
    background: var(--message-user-action-color);
    border: 1px solid var(--fond);
    box-sizing: border-box;
  }
  .nw { left: calc(-13px * var(--hs));  top: calc(-13px * var(--hs));    cursor: nwse-resize; }
  .n  { left: 50%;  top: calc(-13px * var(--hs));    transform: translateX(-50%); cursor: ns-resize; }
  .ne { right: calc(-13px * var(--hs)); top: calc(-13px * var(--hs));    cursor: nesw-resize; }
  .e  { right: calc(-13px * var(--hs)); top: 50%;    transform: translateY(-50%); cursor: ew-resize; }
  .se { right: calc(-13px * var(--hs)); bottom: calc(-13px * var(--hs)); cursor: nwse-resize; }
  .s  { left: 50%;  bottom: calc(-13px * var(--hs)); transform: translateX(-50%); cursor: ns-resize; }
  .sw { left: calc(-13px * var(--hs));  bottom: calc(-13px * var(--hs)); cursor: nesw-resize; }
  .w  { left: calc(-13px * var(--hs));  top: 50%;    transform: translateY(-50%); cursor: ew-resize; }
</style>
