<script lang="ts">
  // Mini-canvas de position d'un overlay — drag/resize + 6 presets + test.
  // Extrait d'InteractionViewerModal (trois squelettes dupliqués) pour être
  // réutilisé par la modale « Créer votre layout » (pop des splits).
  //
  // Référence = canvas RÉEL de la scène (`canvasW/H`), plus de 1920×1080 en
  // dur : sur un canvas 2560×1440, un preset « bas-droite » sortait du
  // mini-canvas et le drag plafonnait à x = 1920.
  //
  // Le composant ne persiste rien : il remonte la config au parent via
  // `onCommit` (qui choisit le store Rust : partagé, chrono ou splitpop).
  import { sceneStore } from "../stores/scene";
  import { t } from "../stores/langue";
  import type { PositionOverlayConfig } from "../tauri";

  let {
    classe = "",
    label,
    config,
    hint,
    onCommit,
    onTester,
  }: {
    classe?: string;
    label: string;
    config: PositionOverlayConfig;
    hint: string;
    onCommit: (cfg: PositionOverlayConfig) => void | Promise<void>;
    onTester?: () => void | Promise<void>;
  } = $props();

  const PRESETS: Record<string, string> = {
    "haut-gauche": "↖",
    "haut-centre": "↑",
    "haut-droite": "↗",
    "bas-gauche": "↙",
    "bas-centre": "↓",
    "bas-droite": "↘",
  };

  let canvasW = $derived($sceneStore.canvasW ?? 1920);
  let canvasH = $derived($sceneStore.canvasH ?? 1080);

  // Dimensions réelles du mini-canvas (mesurées via bind:clientWidth).
  let miniW = $state(300);
  let scale = $derived(miniW / canvasW);

  // Valeurs locales (édition fluide pendant le drag, commit au pointerup).
  // Initialisées neutres : le $effect ci-dessous copie la config du parent
  // dès le montage (lire `config` ici déclencherait
  // `state_referenced_locally` — la copie est volontaire, pas un dérivé).
  let r = $state({ x: 0, y: 0, w: 320, h: 352 });
  let dragging = $state(false);
  // Sync depuis la config du parent quand elle change (sans écraser le drag).
  // Borne aussi au canvas : une config héritée de l'ancien éditeur 1920×1080
  // peut sortir d'un canvas plus petit (ex. y=920 sur 768) — le squelette
  // doit rester visible et déplaçable.
  $effect(() => {
    if (!dragging) {
      r.w = Math.min(config.largeur, canvasW);
      r.h = Math.min(config.hauteur, canvasH);
      r.x = Math.max(0, Math.min(canvasW - r.w, config.x));
      r.y = Math.max(0, Math.min(canvasH - r.h, config.y));
    }
  });

  async function commit() {
    await onCommit({
      x: Math.round(r.x),
      y: Math.round(r.y),
      largeur: Math.round(r.w),
      hauteur: Math.round(r.h),
    });
  }

  function preset(pos: string) {
    const marge = 40;
    r.x = pos.includes("gauche")
      ? marge
      : pos.includes("droite")
        ? canvasW - r.w - marge
        : Math.round((canvasW - r.w) / 2);
    r.y = pos.startsWith("haut") ? marge : canvasH - r.h - marge;
    void commit();
  }

  type DragMode = "move" | "resize-se" | "resize-sw" | "resize-ne" | "resize-nw" | null;
  let dragMode = $state<DragMode>(null);
  let dragStart = { px: 0, py: 0, x: 0, y: 0, w: 0, h: 0 };

  function down(e: PointerEvent, mode: DragMode) {
    e.stopPropagation();
    e.preventDefault();
    dragMode = mode;
    dragging = true;
    dragStart = { px: e.clientX, py: e.clientY, x: r.x, y: r.y, w: r.w, h: r.h };
    (e.target as HTMLElement).setPointerCapture(e.pointerId);
  }

  function move(e: PointerEvent) {
    if (!dragMode) return;
    const dx = (e.clientX - dragStart.px) / scale;
    const dy = (e.clientY - dragStart.py) / scale;

    if (dragMode === "move") {
      r.x = Math.max(0, Math.min(canvasW - r.w, dragStart.x + dx));
      r.y = Math.max(0, Math.min(canvasH - r.h, dragStart.y + dy));
    } else if (dragMode === "resize-se") {
      // Rectangle libre : largeur et hauteur suivent indépendamment la souris.
      r.w = Math.max(80, Math.min(canvasW - r.x, dragStart.w + dx));
      r.h = Math.max(60, Math.min(canvasH - r.y, dragStart.h + dy));
    } else if (dragMode === "resize-sw") {
      // Bord droit fixe : la largeur ne peut pas pousser x sous 0.
      const droite = dragStart.x + dragStart.w;
      r.w = Math.min(droite, Math.max(80, dragStart.w - dx));
      r.x = droite - r.w;
      r.h = Math.max(60, Math.min(canvasH - r.y, dragStart.h + dy));
    } else if (dragMode === "resize-ne") {
      // Bord bas fixe : la hauteur ne peut pas pousser y sous 0.
      const bas = dragStart.y + dragStart.h;
      r.h = Math.min(bas, Math.max(60, dragStart.h - dy));
      r.y = bas - r.h;
      r.w = Math.max(80, Math.min(canvasW - r.x, dragStart.w + dx));
    } else if (dragMode === "resize-nw") {
      const droite = dragStart.x + dragStart.w;
      const bas = dragStart.y + dragStart.h;
      r.w = Math.min(droite, Math.max(80, dragStart.w - dx));
      r.x = droite - r.w;
      r.h = Math.min(bas, Math.max(60, dragStart.h - dy));
      r.y = bas - r.h;
    }
  }

  async function up() {
    if (!dragMode) return;
    dragMode = null;
    dragging = false;
    await commit();
  }
</script>

<div class="sk-presets">
  {#each Object.entries(PRESETS) as [pos, glyph] (pos)}
    <button class="sk-btn" title={$t("interactions.position.pos." + pos)} onclick={() => preset(pos)}>{glyph}</button>
  {/each}
</div>
{#if onTester}
  <button class="sk-btn sk-test" onclick={() => void onTester?.()}>{$t("interactions.position.tester")}</button>
{/if}
<div
  class="sk-canvas"
  bind:clientWidth={miniW}
  style:height="{canvasH * scale}px"
  onpointermove={move}
  onpointerup={up}
  role="presentation"
>
  <div
    class="sk-rect {classe}"
    style="left: {r.x * scale}px; top: {r.y * scale}px; width: {r.w * scale}px; height: {r.h * scale}px;"
    onpointerdown={(e) => down(e, "move")}
    role="presentation"
  >
    <span class="sk-label">{label}</span>
    <div class="sk-handle nw" onpointerdown={(e) => down(e, "resize-nw")} role="presentation"></div>
    <div class="sk-handle ne" onpointerdown={(e) => down(e, "resize-ne")} role="presentation"></div>
    <div class="sk-handle sw" onpointerdown={(e) => down(e, "resize-sw")} role="presentation"></div>
    <div class="sk-handle se" onpointerdown={(e) => down(e, "resize-se")} role="presentation"></div>
  </div>
</div>
<p class="hint">{hint}</p>
<div class="sk-grid">
  <label class="sk-field">
    <span class="field-label">X</span>
    <input
      type="number"
      value={Math.round(r.x)}
      onchange={(e) => { r.x = +(e.target as HTMLInputElement).value; void commit(); }}
    />
  </label>
  <label class="sk-field">
    <span class="field-label">Y</span>
    <input
      type="number"
      value={Math.round(r.y)}
      onchange={(e) => { r.y = +(e.target as HTMLInputElement).value; void commit(); }}
    />
  </label>
  <label class="sk-field">
    <span class="field-label">{$t("commun.largeur")}</span>
    <input
      type="number"
      min="80"
      max={canvasW}
      value={Math.round(r.w)}
      onchange={(e) => { r.w = Math.max(80, +(e.target as HTMLInputElement).value); void commit(); }}
    />
  </label>
  <label class="sk-field">
    <span class="field-label">{$t("interactions.position.hauteurRef")}</span>
    <input
      type="number"
      min="60"
      max={canvasH}
      value={Math.round(r.h)}
      onchange={(e) => { r.h = Math.max(60, +(e.target as HTMLInputElement).value); void commit(); }}
    />
  </label>
</div>

<style>
  .sk-presets {
    display: grid;
    grid-template-columns: repeat(6, 1fr);
    gap: 0.25rem;
    margin-bottom: 0.4rem;
  }
  .sk-btn {
    background: var(--btn-surface);
    box-shadow: var(--btn-inset);
    color: var(--texte);
    border: 1px solid var(--bordure-ui);
    border-radius: var(--rayon-petit);
    padding: 0.15rem 0.5rem;
    font: inherit;
    font-size: 0.75rem;
    cursor: pointer;
    transition: background 0.15s ease, box-shadow 0.15s ease, border-color 0.15s ease;
  }
  .sk-btn:hover {
    background: var(--btn-surface-hover);
    box-shadow: var(--btn-inset-hover);
    border-color: var(--accent-ui);
  }
  .sk-presets .sk-btn {
    padding: 0.15rem 0;
    text-align: center;
  }
  .sk-test {
    width: 100%;
    margin-bottom: 0.4rem;
  }
  /* Mini-canvas : ratio du canvas RÉEL (hauteur posée en style). */
  .sk-canvas {
    position: relative;
    width: 100%;
    background: var(--fond);
    border: 1px solid var(--bordure-ui);
    overflow: hidden;
    touch-action: none;
    user-select: none;
  }
  .sk-rect {
    position: absolute;
    background: color-mix(in srgb, var(--accent-ui) 12%, transparent);
    border: 2px solid var(--bordure-ui-active);
    cursor: move;
    display: flex;
    align-items: center;
    justify-content: center;
    touch-action: none;
  }
  .sk-rect:hover {
    background: color-mix(in srgb, var(--accent-ui) 18%, transparent);
  }
  /* Squelette DÉDIÉ du chrono 2 — teinte ambre pour le distinguer du
     squelette partagé (accueil/alertes/commandes/pad). */
  .sk-rect.squelette-chrono {
    background: color-mix(in srgb, var(--message-user-action-color) 12%, transparent);
    border-color: var(--message-user-action-color);
  }
  .sk-rect.squelette-chrono:hover {
    background: color-mix(in srgb, var(--message-user-action-color) 18%, transparent);
  }
  /* Squelette DÉDIÉ du pop de validation des splits — teinte verte
     (couleur des gains speedrun) pour le distinguer du partagé et du
     chrono 2 (ambre). */
  .sk-rect.squelette-splitpop {
    background: color-mix(in srgb, var(--message-ok-color) 12%, transparent);
    border-color: var(--message-ok-color);
  }
  .sk-rect.squelette-splitpop:hover {
    background: color-mix(in srgb, var(--message-ok-color) 18%, transparent);
  }
  .sk-label {
    font-size: 0.7rem;
    opacity: 0.6;
    pointer-events: none;
  }
  .sk-handle {
    position: absolute;
    width: 10px;
    height: 10px;
    background: var(--accent-ui);
    border: 1px solid var(--fond);
  }
  .sk-handle.nw { top: -6px; left: -6px; cursor: nwse-resize; }
  .sk-handle.ne { top: -6px; right: -6px; cursor: nesw-resize; }
  .sk-handle.sw { bottom: -6px; left: -6px; cursor: nesw-resize; }
  .sk-handle.se { bottom: -6px; right: -6px; cursor: nwse-resize; }
  .sk-grid {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 0.4rem;
  }
  .sk-field {
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
    min-width: 0;
  }
  .sk-field input {
    background: var(--fond-controle);
    color: var(--texte);
    border: 1px solid var(--bordure-ui);
    border-radius: var(--rayon-petit);
    padding: 0.25rem 0.4rem;
    font: inherit;
    font-size: 0.8rem;
    width: 100%;
  }
</style>
