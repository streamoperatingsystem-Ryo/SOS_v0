<script lang="ts">
  import { onMount } from "svelte";
  import { sceneStore, selectWidget } from "../stores/scene";
  import { openSectionExplicit } from "../stores/ui";
  import { registerVideo, unregisterVideo } from "../stores/video";
  import WidgetComp from "./Widget.svelte";
  import AlignmentGuides from "./AlignmentGuides.svelte";
  import CadreSVG from "./CadreSVG.svelte";

  // Dims canvas = résolution OBS lue au connect (fallback 1920×1080).
  // Widgets existants jamais rescalés quand ces dims changent.
  let canvasW = $derived($sceneStore.canvasW ?? 1920);
  let canvasH = $derived($sceneStore.canvasH ?? 1080);

  let containerEl: HTMLDivElement;
  let availW = $state(0);
  let availH = $state(0);

  let scale = $derived(
    availW && availH
      ? Math.min(availW / canvasW, availH / canvasH)
      : 0
  );

  // ===== Fond de scène =====
  // Préfixe URL pour les médias servis par le serveur :4321 (dashboard ≠ :4321).
  const MEDIA_BASE = "http://127.0.0.1:4321/";

  let bgMedia = $derived($sceneStore.bgMedia ?? "");
  let bgKind = $derived($sceneStore.bgKind ?? "image");
  let bgFit = $derived($sceneStore.bgFit ?? "remplir");
  let bgZoom = $derived($sceneStore.bgZoom ?? 1);
  let bgRot = $derived($sceneStore.bgRot ?? 0);
  let hasFond = $derived(bgMedia.length > 0);
  let bgIsVideo = $derived(bgKind === "video");

  // Mapping mode → object-fit (identique à Widget.svelte).
  const FIT_CSS: Record<string, string> = {
    ajuster: "contain",
    remplir: "cover",
    etirer: "fill",
    centrer: "none",
    vignette: "scale-down",
    etendre: "cover",
  };
  let bgFitCss = $derived(FIT_CSS[bgFit] ?? "cover");
  let bgMediaTransform = $derived(`rotate(${bgRot}deg) scale(${bgZoom})`);

  // ===== Cadre de l'application (bord extérieur du canvas) =====
  let cadreApp = $derived($sceneStore.cadreApp ?? null);
  let cadreAppActif = $derived(!!cadreApp?.actif && (cadreApp?.strokeWidth ?? 0) > 0);

  // <video> fond src set impérativement — Svelte ne touche JAMAIS à l'attribut
  // src du template. On ne set src QUE si bgMedia change (nom de fichier).
  // Drag/resize/rotate = style only, currentTime préservé.
  let bgVideoEl: HTMLVideoElement | undefined = $state(undefined);
  $effect(() => {
    if (!bgVideoEl || !bgMedia) return;
    const newSrc = MEDIA_BASE + bgMedia;
    if (bgVideoEl.src !== newSrc) {
      bgVideoEl.src = newSrc;
    }
  });

  // Dashboard : aucune vidéo en lecture par défaut. preload="metadata" +
  // paused + currentTime = 0 (vignette). La barre lecteur (Toolbar) pilote
  // :4321 via bgPaused/bgTime (snapshot WS) — pas le dashboard.
  // $effect bgTime : applique la position une fois pour la vignette.
  $effect(() => {
    if (!bgVideoEl) return;
    registerVideo("fond", bgVideoEl);
    return () => unregisterVideo("fond");
  });
  $effect(() => {
    if (!bgVideoEl) return;
    const t = $sceneStore.bgTime ?? 0;
    if (Number.isFinite(t) && Math.abs(bgVideoEl.currentTime - t) > 0.05) {
      bgVideoEl.currentTime = t;
    }
  });

  // Clic canvas vide = désélection + ouvre section Scène.
  function onCanvasPointerDown() {
    selectWidget(null);
    openSectionExplicit("scene");
  }

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
      style="width:{canvasW}px; height:{canvasH}px; transform: scale({scale});"
      onpointerdown={onCanvasPointerDown}
      role="presentation"
    >
      {#if hasFond}
        <div class="bg-layer">
          {#if bgIsVideo}
            <video
              class="bg-media bg-video"
              bind:this={bgVideoEl}
              muted
              playsinline
              preload="metadata"
              draggable="false"
              style="object-fit:{bgFitCss}; object-position:{bgFit === 'centrer' ? 'center' : '50% 50%'}; transform:{bgMediaTransform}; transform-origin:center center;"
            ></video>
          {:else}
            <img
              class="bg-media"
              src={MEDIA_BASE + bgMedia}
              alt=""
              draggable="false"
              style="object-fit:{bgFitCss}; object-position:{bgFit === 'centrer' ? 'center' : '50% 50%'}; transform:{bgMediaTransform}; transform-origin:center center;"
            />
          {/if}
        </div>
      {/if}
      {#each $sceneStore.widgets as w (w.id)}
        <WidgetComp {w} {scale} />
      {/each}

      <!-- Cadre de l'application (overlay SVG bord canvas). Au-dessus des
           widgets, pointer-events:none. Visible en dashboard ET diffusion. -->
      {#if cadreAppActif && cadreApp}
        <div class="cadre-app-overlay">
          <CadreSVG
            style={cadreApp.style}
            variante={cadreApp.variante}
            largeur={canvasW}
            hauteur={canvasH}
            strokeWidth={cadreApp.strokeWidth}
            couleur={cadreApp.couleur}
            couleurFin={cadreApp.couleurFin}
            idCadre="app"
          />
        </div>
      {/if}

      <!-- Guides d'alignement (dashboard uniquement — jamais en diffusion).
           4 traits rouges aux bords du widget + labels distance px + viseur
           central (cyan → jaune quand proche du centre). Apparaissent
           pendant le drag/resize d'un widget. -->
      <AlignmentGuides />
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
  .bg-layer {
    position: absolute;
    inset: 0;
    overflow: hidden;
    z-index: 0;
    pointer-events: none;
  }
  .bg-media {
    width: 100%;
    height: 100%;
    display: block;
    pointer-events: none;
  }
  .bg-video {
    pointer-events: none;
  }
  /* Overlay cadre app : bord extérieur du canvas, au-dessus des widgets. */
  .cadre-app-overlay {
    position: absolute;
    inset: 0;
    pointer-events: none;
    z-index: 100;
  }
</style>
