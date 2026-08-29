<script lang="ts">
  // AlignmentGuides — Guides d'alignement type OBS (dashboard uniquement).
  // Dessine sur un <canvas> HTML :
  //   - 4 traits aux bords du widget (gauche, droite, haut, bas)
  //   - Labels avec la distance en px canvas logique
  //   - Viseur central (croix + cercle) — cyan par défaut, JAUNE quand le
  //     widget est proche du centre (snap visuel)
  //   - Label "CENTRE" quand le widget est aligné au centre
  //
  // Rendu à l'intérieur du canvas (coordonnées canvas logiques).
  // pointer-events: none → ne bloque pas les interactions.
  // Portage de AlignmentGuides.svelte du projet référence (RUST_SOS_2026v0.1).
  import { onMount } from "svelte";
  import { guidesEtat } from "../stores/guides";
  import { sceneStore } from "../stores/scene";

  let canvasW = $derived($sceneStore.canvasW ?? 1920);
  let canvasH = $derived($sceneStore.canvasH ?? 1080);

  let canvasEl: HTMLCanvasElement;
  let ctx: CanvasRenderingContext2D | null = null;

  onMount(() => {
    if (canvasEl) {
      canvasEl.width = canvasW;
      canvasEl.height = canvasH;
      ctx = canvasEl.getContext("2d");
    }
  });

  // Redessiner à chaque changement d'état ou de dims canvas.
  $effect(() => {
    if (canvasEl && (canvasEl.width !== canvasW || canvasEl.height !== canvasH)) {
      canvasEl.width = canvasW;
      canvasEl.height = canvasH;
    }
    if (ctx && $guidesEtat) {
      dessinerGuides($guidesEtat);
    }
  });

  function dessinerGuides(etat: typeof $guidesEtat): void {
    if (!ctx) return;
    ctx.clearRect(0, 0, canvasW, canvasH);

    if (!etat.afficherGuides || !etat.visible || !etat.widgetRect) return;

    const widgetRect = etat.widgetRect;

    // Styles communs
    const couleurTrait = "#e74c3c";
    const epaisseurTrait = 2;
    const couleurLabel = "#ffffff";
    const couleurFondLabel = "rgba(0, 0, 0, 0.75)";
    const taillePolice = 12;

    ctx.strokeStyle = couleurTrait;
    ctx.lineWidth = epaisseurTrait;
    ctx.font = `${taillePolice}px sans-serif`;
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";

    const pad = 6;
    const labelHeight = taillePolice + pad * 2;

    // Coordonnées des bords du widget
    const wLeft = widgetRect.x;
    const wTop = widgetRect.y;
    const wRight = widgetRect.x + widgetRect.width;
    const wBottom = widgetRect.y + widgetRect.height;

    // Distances en px canvas logiques
    const leftPx = Math.round(wLeft);
    const topPx = Math.round(wTop);
    const rightPx = Math.round(canvasW - wRight);
    const bottomPx = Math.round(canvasH - wBottom);

    // Trait vertical gauche
    ctx.beginPath();
    ctx.moveTo(wLeft, 0);
    ctx.lineTo(wLeft, canvasH);
    ctx.stroke();
    drawLabel(`${leftPx} px`, wLeft, pad + labelHeight / 2, couleurLabel, couleurFondLabel);

    // Trait vertical droit
    ctx.beginPath();
    ctx.moveTo(wRight, 0);
    ctx.lineTo(wRight, canvasH);
    ctx.stroke();
    drawLabel(`${rightPx} px`, wRight, canvasH - pad - labelHeight / 2, couleurLabel, couleurFondLabel);

    // Trait horizontal haut
    ctx.beginPath();
    ctx.moveTo(0, wTop);
    ctx.lineTo(canvasW, wTop);
    ctx.stroke();
    drawLabel(`${topPx} px`, pad + 36, wTop, couleurLabel, couleurFondLabel);

    // Trait horizontal bas
    ctx.beginPath();
    ctx.moveTo(0, wBottom);
    ctx.lineTo(canvasW, wBottom);
    ctx.stroke();
    drawLabel(`${bottomPx} px`, canvasW - pad - 36, wBottom, couleurLabel, couleurFondLabel);

    // --- Viseur de centrage ---
    const cx = canvasW / 2;
    const cy = canvasH / 2;
    const elCx = wLeft + widgetRect.width / 2;
    const elCy = wTop + widgetRect.height / 2;
    const procheCx = Math.abs(elCx - cx) < 6;
    const procheCy = Math.abs(elCy - cy) < 6;
    const proche = procheCx || procheCy;

    const couleurViseur = proche ? "#FFEE00" : "#00E5FF";
    ctx.save();
    ctx.strokeStyle = couleurViseur;
    ctx.lineWidth = proche ? 2.5 : 1.5;
    ctx.globalAlpha = proche ? 0.95 : 0.6;

    // Trait vertical centre
    ctx.beginPath();
    ctx.moveTo(cx, 0);
    ctx.lineTo(cx, canvasH);
    ctx.stroke();

    // Trait horizontal centre
    ctx.beginPath();
    ctx.moveTo(0, cy);
    ctx.lineTo(canvasW, cy);
    ctx.stroke();

    // Cercle au centre
    ctx.beginPath();
    ctx.arc(cx, cy, proche ? 10 : 6, 0, Math.PI * 2);
    ctx.stroke();

    ctx.restore();

    if (proche) {
      ctx.save();
      ctx.fillStyle = couleurViseur;
      ctx.strokeStyle = "#000";
      ctx.lineWidth = 3;
      ctx.font = "bold 11px sans-serif";
      ctx.textAlign = "center";
      ctx.textBaseline = "middle";
      const tx = cx + 22;
      const ty = cy - 14;
      const texteCentre = "CENTRE";
      ctx.strokeText(texteCentre, tx, ty);
      ctx.fillText(texteCentre, tx, ty);
      ctx.restore();
    }
  }

  function drawLabel(text: string, x: number, y: number, couleurLabel: string, couleurFondLabel: string): void {
    if (!ctx) return;
    const pad = 6;
    const m = ctx.measureText(text);
    const tw = m.width + pad * 2;
    const th = 12 + pad * 2;
    const left = x - tw / 2;
    const top = y - th / 2;
    ctx.fillStyle = couleurFondLabel;
    ctx.fillRect(left, top, tw, th);
    ctx.fillStyle = couleurLabel;
    ctx.fillText(text, x, y);
  }
</script>

<canvas
  bind:this={canvasEl}
  class="alignment-guides-canvas"
  style="width: {canvasW}px; height: {canvasH}px;"
></canvas>

<style>
  .alignment-guides-canvas {
    position: absolute;
    inset: 0;
    pointer-events: none;
    z-index: 50;
  }
</style>
