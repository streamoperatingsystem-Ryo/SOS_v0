<script lang="ts">
  import { sceneStore, selectedIdStore, rotateWidgetLocal, rotateFondLocal, commitScene } from "../stores/scene";
  import { get } from "svelte/store";
  import { t } from "../stores/langue";

  // Carré 120×120. Boule positionnée par rotateX/rotateY (±maxDeg).
  // maxDeg=90 (défaut) = inclinaison seule ; maxDeg=180 = mode miroir
  // (passé ±90° le widget montre son dos → miroir H à rotateY=±180,
  //  miroir V à rotateX=±180 — perspective(800px), pas de backface:hidden).
  // cx = 60 + (ry/maxDeg)*60  (droite = rotateY positif)
  // cy = 60 - (rx/maxDeg)*60  (haut   = rotateX positif)
  // cible : "widget" (défaut, pilote le widget sélectionné) ou "fond"
  // (pilote bgRotX/bgRotY de la scène — tilt 3D du fond de l'application).
  let { maxDeg = 90, cible = "widget" }: { maxDeg?: number; cible?: "widget" | "fond" } = $props();
  const SIZE = 120;
  const HALF = SIZE / 2;
  const AXIS_SNAP_DEG = 8; // aimant indépendant par axe (rx=0 ou ry=0)
  const CENTER_SNAP_DEG = 12; // zone plus large au centre (intersection des deux)

  let selectedId = $derived($selectedIdStore);
  let widget = $derived(
    selectedId
      ? $sceneStore.widgets.find((w) => w.id === selectedId) ?? null
      : null
  );
  let rx = $derived(cible === "fond" ? ($sceneStore.bgRotX ?? 0) : (widget?.rotateX ?? 0));
  let ry = $derived(cible === "fond" ? ($sceneStore.bgRotY ?? 0) : (widget?.rotateY ?? 0));

  // Croix pleine si au centre (0,0), sinon axes atténués.
  let atCenter = $derived(Math.abs(rx) < 0.5 && Math.abs(ry) < 0.5);
  // Aimant axe : la boule est verrouillée sur un axe (valeur 0 sur cet axe).
  let snappedX = $derived(Math.abs(ry) < 0.5); // boule sur l'axe horizontal (ry=0)
  let snappedY = $derived(Math.abs(rx) < 0.5); // boule sur l'axe vertical (rx=0)

  let ballCx = $derived(HALF + (ry / maxDeg) * HALF);
  let ballCy = $derived(HALF - (rx / maxDeg) * HALF);

  // Retournement (uniquement possible si maxDeg > 90) : |angle| > 90 → le
  // widget montre son dos = miroir. Indication textuelle sous le gizmo.
  let miroirH = $derived(Math.abs(ry) > 90);
  let miroirV = $derived(Math.abs(rx) > 90);

  let dragging = $state(false);
  let gizmoEl = $state<HTMLDivElement | undefined>(undefined);

  function ptrToDeg(e: PointerEvent): { rx: number; ry: number } {
    if (!gizmoEl) return { rx: 0, ry: 0 };
    const r = gizmoEl.getBoundingClientRect();
    const x = e.clientX - r.left; // 0..SIZE
    const y = e.clientY - r.top; // 0..SIZE
    let nry = ((x - HALF) / HALF) * maxDeg;
    let nrx = ((HALF - y) / HALF) * maxDeg;
    nrx = Math.max(-maxDeg, Math.min(maxDeg, nrx));
    nry = Math.max(-maxDeg, Math.min(maxDeg, nry));
    // Aimant axes : chaque axe snap indépendamment à 0.
    if (Math.abs(nrx) < AXIS_SNAP_DEG) nrx = 0;
    if (Math.abs(nry) < AXIS_SNAP_DEG) nry = 0;
    // Aimant centre : zone plus large, force les deux à 0 (intersection).
    if (Math.abs(nrx) < CENTER_SNAP_DEG && Math.abs(nry) < CENTER_SNAP_DEG) {
      nrx = 0;
      nry = 0;
    }
    // Aimant bord : snap à ±maxDeg (miroir exact à 180° en mode miroir).
    if (Math.abs(Math.abs(nrx) - maxDeg) < AXIS_SNAP_DEG) nrx = Math.sign(nrx) * maxDeg;
    if (Math.abs(Math.abs(nry) - maxDeg) < AXIS_SNAP_DEG) nry = Math.sign(nry) * maxDeg;
    return { rx: nrx, ry: nry };
  }

  // Applique le tilt : widget sélectionné (rotateX/rotateY) ou fond de la
  // scène (bgRotX/bgRotY) selon la cible. Maj locale seule — le commit
  // part au pointerup / dblclick.
  function appliquerTilt(nrx: number, nry: number) {
    if (cible === "fond") {
      rotateFondLocal(nrx, nry);
      return;
    }
    const id = get(selectedIdStore);
    if (id) rotateWidgetLocal(id, nrx, nry);
  }

  function onpointerdown(e: PointerEvent) {
    e.stopPropagation();
    if (cible === "widget" && !get(selectedIdStore)) return;
    dragging = true;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    const { rx: nrx, ry: nry } = ptrToDeg(e);
    appliquerTilt(nrx, nry);
  }

  function onpointermove(e: PointerEvent) {
    if (!dragging) return;
    const { rx: nrx, ry: nry } = ptrToDeg(e);
    appliquerTilt(nrx, nry);
  }

  async function onpointerup(e: PointerEvent) {
    if (!dragging) return;
    dragging = false;
    (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId);
    await commitScene();
  }

  async function ondblclick(e: MouseEvent) {
    e.stopPropagation();
    if (cible === "widget" && !get(selectedIdStore)) return;
    appliquerTilt(0, 0);
    await commitScene();
  }
</script>

{#if cible === "fond" || widget}
  <div class="gizmo-wrap">
    <div
      class="gizmo"
      bind:this={gizmoEl}
      onpointerdown={onpointerdown}
      onpointermove={onpointermove}
      onpointerup={onpointerup}
      ondblclick={ondblclick}
      role="presentation"
    >
      <svg width={SIZE} height={SIZE} viewBox="0 0 {SIZE} {SIZE}">
        <!-- Carré : rouge --ctrl-arreter -->
        <rect
          x="0.5"
          y="0.5"
          width={SIZE - 1}
          height={SIZE - 1}
          fill="none"
          stroke="var(--ctrl-arreter)"
          stroke-width="1"
        />
        <!-- Croix : axes vertical + horizontal, orange --ctrl-redemarrer -->
        <line
          x1={HALF}
          y1="0"
          x2={HALF}
          y2={SIZE}
          stroke="var(--ctrl-redemarrer)"
          stroke-width={snappedY || atCenter ? 1.5 : 1}
          opacity={snappedY || atCenter ? 1 : 0.4}
        />
        <line
          x1="0"
          y1={HALF}
          x2={SIZE}
          y2={HALF}
          stroke="var(--ctrl-redemarrer)"
          stroke-width={snappedX || atCenter ? 1.5 : 1}
          opacity={snappedX || atCenter ? 1 : 0.4}
        />
        <!-- Boule : vert --ctrl-refresh, jaune --ctrl-centre quand centrée -->
        <circle
          cx={ballCx}
          cy={ballCy}
          r={atCenter ? 8 : 6}
          fill={atCenter ? "var(--ctrl-centre)" : "var(--ctrl-refresh)"}
        />
      </svg>
    </div>
    <div class="labels">
      <span>{$t("gizmo.hautBas", { deg: Math.round(rx) })}</span>
      <span>{$t("gizmo.gaucheDroite", { deg: Math.round(ry) })}</span>
      {#if miroirH || miroirV}
        <span class="label-miroir">
          {$t(miroirH && miroirV ? "gizmo.miroirHV" : miroirH ? "gizmo.miroirH" : "gizmo.miroirV")}
        </span>
      {/if}
    </div>
  </div>
{/if}

<style>
  .gizmo-wrap {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.3rem;
    padding: 0.4rem 0;
  }
  .gizmo {
    width: 120px;
    height: 120px;
    cursor: grab;
    touch-action: none;
    user-select: none;
  }
  .gizmo:active {
    cursor: grabbing;
  }
  .labels {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.1rem;
    font-size: 0.75rem;
    opacity: 0.8;
    font-variant-numeric: tabular-nums;
  }
  .label-miroir {
    color: var(--ctrl-centre);
    font-weight: 600;
    opacity: 1;
  }
</style>
