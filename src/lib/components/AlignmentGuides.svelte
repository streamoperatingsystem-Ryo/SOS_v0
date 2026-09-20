<script lang="ts">
  // AlignmentGuides — Guides d'alignement type OBS (dashboard uniquement).
  // Dessine sur un <canvas> HTML :
  //   - 4 traits aux bords du widget (gauche, droite, haut, bas)
  //   - Labels avec la distance en px canvas logique — surlignés jaune
  //     (--ctrl-centre) quand les marges opposées sont égales
  //   - Viseur central (croix + cercle) — vert --ctrl-refresh par défaut,
  //     JAUNE --ctrl-centre quand le widget est proche du centre (snap visuel)
  //   - Label "CENTRE" quand le widget est aligné au centre
  //   - Fantôme miroir : contour pointillé à la position symétrique du widget
  //     (axe vertical du canvas) — visible tant qu'elle diffère de la position
  //   - Espacements égaux : segments entre widgets quand le gap créé par le
  //     widget déplacé égale un gap existant entre deux autres widgets
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

  // Lit une variable CSS :root (canvas JS ne supporte pas var() directement).
  function cssVar(nom: string, fallback: string): string {
    const v = getComputedStyle(document.documentElement).getPropertyValue(nom).trim();
    return v || fallback;
  }

  function dessinerGuides(etat: typeof $guidesEtat): void {
    if (!ctx) return;
    ctx.clearRect(0, 0, canvasW, canvasH);

    if (!etat.afficherGuides || !etat.visible || !etat.widgetRect) return;

    const widgetRect = etat.widgetRect;

    // Styles communs — couleurs centralisées --ctrl-* (app.css :root).
    const couleurTrait = cssVar("--ctrl-arreter", "#c0392b");
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

    // Marges opposées égales (tolérance 0.5px — le snap produit l'égalité
    // exacte) → labels surlignés --ctrl-centre au lieu de blanc.
    const couleurEq = cssVar("--ctrl-centre", "#FFEE00");
    const eqX = Math.abs(wLeft - (canvasW - wRight)) < 0.5;
    const eqY = Math.abs(wTop - (canvasH - wBottom)) < 0.5;
    const labX = eqX ? couleurEq : couleurLabel;
    const labY = eqY ? couleurEq : couleurLabel;

    // Trait vertical gauche
    ctx.beginPath();
    ctx.moveTo(wLeft, 0);
    ctx.lineTo(wLeft, canvasH);
    ctx.stroke();
    drawLabel(`${leftPx} px`, wLeft, pad + labelHeight / 2, labX, couleurFondLabel);

    // Trait vertical droit
    ctx.beginPath();
    ctx.moveTo(wRight, 0);
    ctx.lineTo(wRight, canvasH);
    ctx.stroke();
    drawLabel(`${rightPx} px`, wRight, canvasH - pad - labelHeight / 2, labX, couleurFondLabel);

    // Trait horizontal haut
    ctx.beginPath();
    ctx.moveTo(0, wTop);
    ctx.lineTo(canvasW, wTop);
    ctx.stroke();
    drawLabel(`${topPx} px`, pad + 36, wTop, labY, couleurFondLabel);

    // Trait horizontal bas
    ctx.beginPath();
    ctx.moveTo(0, wBottom);
    ctx.lineTo(canvasW, wBottom);
    ctx.stroke();
    drawLabel(`${bottomPx} px`, canvasW - pad - 36, wBottom, labY, couleurFondLabel);

    // --- Viseur de centrage ---
    const cx = canvasW / 2;
    const cy = canvasH / 2;
    const elCx = wLeft + widgetRect.width / 2;
    const elCy = wTop + widgetRect.height / 2;
    const procheCx = Math.abs(elCx - cx) < 6;
    const procheCy = Math.abs(elCy - cy) < 6;
    const proche = procheCx || procheCy;

    const couleurViseur = proche
      ? cssVar("--ctrl-centre", "#FFEE00")
      : cssVar("--ctrl-refresh", "#22c55e");
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

    // --- Fantôme miroir (axe vertical du canvas) ---
    // Contour pointillé vert à la position symétrique du widget : montre « ce
    // que ça enlève à marge » de l'autre côté. Masqué si centré (= même place).
    const ghostX = canvasW - wLeft - widgetRect.width;
    if (Math.abs(ghostX - wLeft) > 1) {
      ctx.save();
      ctx.strokeStyle = cssVar("--ctrl-refresh", "#22c55e");
      ctx.globalAlpha = 0.5;
      ctx.setLineDash([6, 4]);
      ctx.strokeRect(ghostX, wTop, widgetRect.width, widgetRect.height);
      ctx.setLineDash([]);
      ctx.restore();
    }

    // --- Espacements égaux (cohérence entre widgets) ---
    // Autres widgets = tous sauf celui déplacé (exclu par égalité de rect : le
    // store reçoit les mêmes valeurs snappées que widgetRect pendant le drag).
    const autres = $sceneStore.widgets.filter(
      (o) =>
        !(
          Math.abs(o.x - wLeft) < 0.5 &&
          Math.abs(o.y - wTop) < 0.5 &&
          Math.abs(o.largeur - widgetRect.width) < 0.5 &&
          Math.abs(o.hauteur - widgetRect.height) < 0.5
        )
    );
    // Gaps de référence : paires d'autres widgets non chevauchants, avec
    // recouvrement sur l'axe perpendiculaire (le gap n'a de sens qu'aligné).
    type GapSeg = { x1: number; y1: number; x2: number; y2: number; g: number };
    const refGapsX: GapSeg[] = [];
    const refGapsY: GapSeg[] = [];
    for (const a of autres) {
      for (const b of autres) {
        if (a === b) continue;
        const gX = b.x - (a.x + a.largeur);
        const ovTx = Math.max(a.y, b.y);
        const ovBx = Math.min(a.y + a.hauteur, b.y + b.hauteur);
        if (gX > 0 && ovBx - ovTx > 0) {
          refGapsX.push({ x1: a.x + a.largeur, y1: (ovTx + ovBx) / 2, x2: b.x, y2: (ovTx + ovBx) / 2, g: gX });
        }
        const gY = b.y - (a.y + a.hauteur);
        const ovLy = Math.max(a.x, b.x);
        const ovRy = Math.min(a.x + a.largeur, b.x + b.largeur);
        if (gY > 0 && ovRy - ovLy > 0) {
          refGapsY.push({ x1: (ovLy + ovRy) / 2, y1: a.y + a.hauteur, x2: (ovLy + ovRy) / 2, y2: b.y, g: gY });
        }
      }
    }
    // Gap entre le widget déplacé et chaque autre widget : s'il égale un gap
    // de référence (±0.5px), on dessine les DEUX segments en jaune + label.
    const couleurGap = cssVar("--ctrl-centre", "#FFEE00");
    const dessinerGap = (seg: GapSeg, ref: GapSeg) => {
      if (!ctx) return;
      ctx.save();
      ctx.strokeStyle = couleurGap;
      ctx.lineWidth = 1.5;
      ctx.setLineDash([4, 3]);
      for (const s of [seg, ref]) {
        ctx.beginPath();
        ctx.moveTo(s.x1, s.y1);
        ctx.lineTo(s.x2, s.y2);
        ctx.stroke();
      }
      ctx.setLineDash([]);
      ctx.restore();
      drawLabel(`${Math.round(seg.g)} px`, (seg.x1 + seg.x2) / 2, (seg.y1 + seg.y2) / 2, couleurGap, couleurFondLabel);
    };
    for (const o of autres) {
      // o à gauche / à droite du déplacé (recouvrement vertical requis).
      const ovT = Math.max(o.y, wTop);
      const ovB = Math.min(o.y + o.hauteur, wBottom);
      if (ovB - ovT > 0) {
        const midY = (ovT + ovB) / 2;
        const gGauche = wLeft - (o.x + o.largeur);
        if (gGauche > 0) {
          const ref = refGapsX.find((p) => Math.abs(p.g - gGauche) < 0.5);
          if (ref) dessinerGap({ x1: o.x + o.largeur, y1: midY, x2: wLeft, y2: midY, g: gGauche }, ref);
        }
        const gDroite = o.x - wRight;
        if (gDroite > 0) {
          const ref = refGapsX.find((p) => Math.abs(p.g - gDroite) < 0.5);
          if (ref) dessinerGap({ x1: wRight, y1: midY, x2: o.x, y2: midY, g: gDroite }, ref);
        }
      }
      // o au-dessus / en dessous du déplacé (recouvrement horizontal requis).
      const ovL = Math.max(o.x, wLeft);
      const ovR = Math.min(o.x + o.largeur, wRight);
      if (ovR - ovL > 0) {
        const midX = (ovL + ovR) / 2;
        const gHaut = wTop - (o.y + o.hauteur);
        if (gHaut > 0) {
          const ref = refGapsY.find((p) => Math.abs(p.g - gHaut) < 0.5);
          if (ref) dessinerGap({ x1: midX, y1: o.y + o.hauteur, x2: midX, y2: wTop, g: gHaut }, ref);
        }
        const gBas = o.y - wBottom;
        if (gBas > 0) {
          const ref = refGapsY.find((p) => Math.abs(p.g - gBas) < 0.5);
          if (ref) dessinerGap({ x1: midX, y1: wBottom, x2: midX, y2: o.y, g: gBas }, ref);
        }
      }
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
