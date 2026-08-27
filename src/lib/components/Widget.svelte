<script lang="ts">
  import {
    moveWidgetLocal,
    resizeWidgetLocal,
    commitScene,
    selectedIdStore,
    selectWidget,
  } from "../stores/scene";
  import { openSectionExplicit } from "../stores/ui";
  import { registerVideo, unregisterVideo } from "../stores/video";
  import type { Widget } from "../tauri";

  let { w, scale }: { w: Widget; scale: number } = $props();

  // Mini widget — jamais en dessous.
  const MIN = 80;

  let outerEl: HTMLDivElement;

  let dragging = $state(false);
  let resizing = $state(false);
  let handle = "";
  let startX = 0;
  let startY = 0;
  let startWX = 0;
  let startWY = 0;
  let startW = 0;
  let startH = 0;

  // Préfixe URL pour les médias servis par le serveur :4321 (dashboard ≠ :4321).
  const MEDIA_BASE = "http://127.0.0.1:4321/";

  let selected = $derived($selectedIdStore === w.id);
  let rx = $derived(w.rotateX ?? 0);
  let ry = $derived(w.rotateY ?? 0);
  let fit = $derived(w.mediaFit ?? "ajuster");
  let zoom = $derived(w.mediaZoom ?? 1);
  let rot = $derived(w.mediaRot ?? 0);
  // Transform centré sur l'élément média (img/video). Le cadre widget et le
  // gizmo 3D ne bougent pas — overflow:hidden coupe le débordement.
  let mediaTransform = $derived(`rotate(${rot}deg) scale(${zoom})`);

  // Kind : depuis le champ, sinon déduit de l'extension (widgets existants).
  const VIDEO_EXT = ["mp4", "webm"];
  function kindFromMedia(rel: string): string {
    const ext = rel.split(".").pop()?.toLowerCase() ?? "";
    return VIDEO_EXT.includes(ext) ? "video" : "image";
  }
  let kind = $derived(w.kind ?? (w.media ? kindFromMedia(w.media) : "image"));
  let isVideo = $derived(kind === "video");
  let isTrou = $derived(w.trou === true);

  // Mapping mode → object-fit. "etendre" géré à part (img auto + min 100%).
  const FIT_CSS: Record<string, string> = {
    ajuster: "contain",
    remplir: "cover",
    etirer: "fill",
    centrer: "none",
    vignette: "scale-down",
    etendre: "cover",
  };
  let fitCss = $derived(FIT_CSS[fit] ?? "contain");

  // Dashboard : aucune vidéo en lecture par défaut. preload="metadata" +
  // paused + currentTime = 0 (vignette). La barre lecteur (Toolbar) pilote
  // :4321 via mediaPaused/mediaTime (snapshot WS) — pas le dashboard.
  // $effect mediaTime : applique la position une fois pour la vignette.
  let videoEl: HTMLVideoElement | undefined = $state(undefined);
  $effect(() => {
    if (!videoEl || !w.media) return;
    const newSrc = MEDIA_BASE + w.media;
    if (videoEl.src !== newSrc) {
      videoEl.src = newSrc;
    }
  });
  $effect(() => {
    if (!videoEl) return;
    registerVideo(w.id, videoEl);
    return () => unregisterVideo(w.id);
  });
  $effect(() => {
    if (!videoEl) return;
    const t = w.mediaTime ?? 0;
    if (Number.isFinite(t) && Math.abs(videoEl.currentTime - t) > 0.05) {
      videoEl.currentTime = t;
    }
  });

  function onpointerdown(e: PointerEvent) {
    e.stopPropagation();
    selectWidget(w.id);
    openSectionExplicit("widgets");
    dragging = true;
    startX = e.clientX;
    startY = e.clientY;
    startWX = w.x;
    startWY = w.y;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }

  function onHandleDown(e: PointerEvent, h: string) {
    e.stopPropagation();
    selectWidget(w.id);
    openSectionExplicit("widgets");
    resizing = true;
    handle = h;
    startX = e.clientX;
    startY = e.clientY;
    startWX = w.x;
    startWY = w.y;
    startW = w.largeur;
    startH = w.hauteur;
    // Capture sur l'outer : move/up écoutés sur l'outer.
    outerEl.setPointerCapture(e.pointerId);
  }

  function onpointermove(e: PointerEvent) {
    if (dragging) {
      const nx = startWX + (e.clientX - startX) / scale;
      const ny = startWY + (e.clientY - startY) / scale;
      moveWidgetLocal(w.id, nx, ny);
      return;
    }
    if (resizing) {
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
      resizeWidgetLocal(w.id, nx, ny, nw, nh);
    }
  }

  async function onpointerup(e: PointerEvent) {
    if (!dragging && !resizing) return;
    dragging = false;
    resizing = false;
    handle = "";
    (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId);
    await commitScene();
  }
</script>

<!-- Outer = hit-box rectangle NON incliné (drag position + poignées resize). -->
<div
  class="widget"
  class:dragging
  bind:this={outerEl}
  style="left:{w.x}px; top:{w.y}px; width:{w.largeur}px; height:{w.hauteur}px; z-index:{w.z};"
  onpointerdown={onpointerdown}
  onpointermove={onpointermove}
  onpointerup={onpointerup}
  role="button"
  tabindex="0"
>
  <!-- Inner = bloc visuel incliné (cadre + image). pointer-events:none. -->
  <div
    class="widget-3d"
    class:selected
    style="transform: perspective(800px) rotateX({rx}deg) rotateY({ry}deg);"
  >
    {#if w.media && !isVideo}
      <img
        class="preview"
        src={MEDIA_BASE + w.media}
        alt=""
        draggable="false"
        style="object-fit:{fitCss}; object-position:{fit === 'centrer' ? 'center' : '50% 50%'}; transform:{mediaTransform}; transform-origin:center center;"
      />
    {:else if w.media && isVideo}
      <video
        class="preview video"
        bind:this={videoEl}
        muted
        playsinline
        preload="metadata"
        draggable="false"
        style="object-fit:{fitCss}; object-position:{fit === 'centrer' ? 'center' : '50% 50%'}; transform:{mediaTransform}; transform-origin:center center;"
      ></video>
    {/if}
  </div>

  <!-- Overlay trou : cadre pointillé par-dessus le widget quand trou activé.
       Le widget reste sélectionnable (l'outer hit-box est intact).
       Tilté avec la même perspective que .widget-3d → correspond au trou :4321. -->
  {#if isTrou}
    <div
      class="trou-overlay"
      style="transform: perspective(800px) rotateX({rx}deg) rotateY({ry}deg);"
    ></div>
  {/if}

  <!-- Poignées resize : 4 coins + 4 bords. Widget sélectionné seulement. -->
  {#if selected}
    <div class="handle nw" onpointerdown={(e) => onHandleDown(e, "nw")}></div>
    <div class="handle n"  onpointerdown={(e) => onHandleDown(e, "n")}></div>
    <div class="handle ne" onpointerdown={(e) => onHandleDown(e, "ne")}></div>
    <div class="handle e"  onpointerdown={(e) => onHandleDown(e, "e")}></div>
    <div class="handle se" onpointerdown={(e) => onHandleDown(e, "se")}></div>
    <div class="handle s"  onpointerdown={(e) => onHandleDown(e, "s")}></div>
    <div class="handle sw" onpointerdown={(e) => onHandleDown(e, "sw")}></div>
    <div class="handle w"  onpointerdown={(e) => onHandleDown(e, "w")}></div>
  {/if}
</div>

<style>
  .widget {
    position: absolute;
    cursor: grab;
    touch-action: none;
    user-select: none;
  }
  .widget.dragging {
    cursor: grabbing;
  }
  .widget-3d {
    width: 100%;
    height: 100%;
    background: rgba(224, 224, 224, 0.05);
    border: 1px solid var(--texte);
    transform-origin: center center;
    pointer-events: none;
    box-sizing: border-box;
    overflow: hidden;
  }
  .widget-3d.selected {
    outline: 2px solid var(--texte);
    outline-offset: -2px;
  }
  .trou-overlay {
    position: absolute;
    inset: 0;
    border: 2px dashed var(--texte);
    pointer-events: none;
    box-sizing: border-box;
    z-index: 2;
  }
  .preview {
    width: 100%;
    height: 100%;
    object-fit: fill;
    display: block;
    pointer-events: none;
  }
  .preview.video {
    pointer-events: none;
  }
  .handle {
    position: absolute;
    width: 8px;
    height: 8px;
    background: var(--texte);
    border: 1px solid var(--fond);
    box-sizing: border-box;
    z-index: 1;
  }
  .nw { left: -4px;  top: -4px;    cursor: nwse-resize; }
  .n  { left: 50%;  top: -4px;    transform: translateX(-50%); cursor: ns-resize; }
  .ne { right: -4px; top: -4px;    cursor: nesw-resize; }
  .e  { right: -4px; top: 50%;    transform: translateY(-50%); cursor: ew-resize; }
  .se { right: -4px; bottom: -4px; cursor: nwse-resize; }
  .s  { left: 50%;  bottom: -4px; transform: translateX(-50%); cursor: ns-resize; }
  .sw { left: -4px;  bottom: -4px; cursor: nesw-resize; }
  .w  { left: -4px;  top: 50%;    transform: translateY(-50%); cursor: ew-resize; }
</style>
