<script lang="ts">
  import { onMount } from "svelte";
  import { sceneStore, selectWidget } from "../stores/scene";
  import WidgetComp from "./Widget.svelte";

  const CANVAS_W = 1920;
  const CANVAS_H = 1080;

  let containerEl: HTMLDivElement;
  let availW = $state(0);
  let availH = $state(0);

  let scale = $derived(
    availW && availH
      ? Math.min(availW / CANVAS_W, availH / CANVAS_H)
      : 0
  );

  onMount(() => {
    const ro = new ResizeObserver((entries) => {
      const r = entries[0].contentRect;
      availW = r.width;
      availH = r.height;
    });
    ro.observe(containerEl);
    return () => ro.disconnect();
  });
</script>

<div class="canvas-wrap" bind:this={containerEl}>
  {#if scale > 0}
    <div
      class="canvas"
      style="width:{CANVAS_W}px; height:{CANVAS_H}px; transform: scale({scale});"
      onpointerdown={() => selectWidget(null)}
      role="presentation"
    >
      {#each $sceneStore.widgets as w (w.id)}
        <WidgetComp {w} {scale} />
      {/each}
    </div>
  {/if}
</div>

<style>
  .canvas-wrap {
    flex: 1;
    min-height: 0;
    min-width: 0;
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
    overflow: hidden;
    padding: 1rem;
  }
  .canvas {
    position: relative;
    transform-origin: center center;
    background: var(--fond);
    border: 1px solid var(--texte);
    flex-shrink: 0;
  }
</style>
