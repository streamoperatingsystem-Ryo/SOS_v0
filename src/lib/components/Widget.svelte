<script lang="ts">
  import { moveWidgetLocal, commitScene, selectedIdStore, selectWidget } from "../stores/scene";
  import type { Widget } from "../tauri";

  let { w, scale }: { w: Widget; scale: number } = $props();

  let dragging = $state(false);
  let startX = 0;
  let startY = 0;
  let startWX = 0;
  let startWY = 0;

  // Préfixe URL pour les médias servis par le serveur :4321 (dashboard ≠ :4321).
  const MEDIA_BASE = "http://127.0.0.1:4321/";

  let selected = $derived($selectedIdStore === w.id);

  function onpointerdown(e: PointerEvent) {
    selectWidget(w.id);
    dragging = true;
    startX = e.clientX;
    startY = e.clientY;
    startWX = w.x;
    startWY = w.y;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }

  function onpointermove(e: PointerEvent) {
    if (!dragging) return;
    const nx = startWX + (e.clientX - startX) / scale;
    const ny = startWY + (e.clientY - startY) / scale;
    moveWidgetLocal(w.id, nx, ny);
  }

  async function onpointerup(e: PointerEvent) {
    if (!dragging) return;
    dragging = false;
    (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId);
    await commitScene();
  }
</script>

<div
  class="widget"
  class:dragging
  class:selected
  style="left:{w.x}px; top:{w.y}px; width:{w.largeur}px; height:{w.hauteur}px; z-index:{w.z};"
  onpointerdown={onpointerdown}
  onpointermove={onpointermove}
  onpointerup={onpointerup}
  role="button"
  tabindex="0"
>
  {#if w.media}
    <img class="preview" src={MEDIA_BASE + w.media} alt="" draggable="false" />
  {/if}
</div>

<style>
  .widget {
    position: absolute;
    background: rgba(224, 224, 224, 0.05);
    border: 1px solid var(--texte);
    cursor: grab;
    touch-action: none;
    user-select: none;
  }
  .widget.dragging {
    cursor: grabbing;
  }
  .widget.selected {
    outline: 2px solid var(--texte);
    outline-offset: -2px;
  }
  .preview {
    width: 100%;
    height: 100%;
    object-fit: fill;
    display: block;
    pointer-events: none;
  }
</style>
