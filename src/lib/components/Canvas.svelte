<script lang="ts">
  import { onMount } from "svelte";
  import { sceneStore, selectWidget, ajouterMorphFond } from "../stores/scene";
  import { setCarteEdition } from "../stores/ui";
  import { registerVideo, unregisterVideo } from "../stores/video";
  import { morphMode, morphIntensite, morphRayon, morphSens } from "../stores/morph";
  import { creerRenduMorph, dessinerFitCanvas, type RenduMorph } from "../morph/gl-morph";
  import WidgetComp from "./Widget.svelte";
  import AlignmentGuides from "./AlignmentGuides.svelte";
  import CadreSVG from "./CadreSVG.svelte";
  import { couleurCadre, gradientCadre } from "../cadres/registre";
  import { POLICE_TITRE_DEFAUT, TAILLE_TITRE_DEFAUT, ESPACEMENT_TITRE_DEFAUT } from "../fonts";

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
  let bgOx = $derived($sceneStore.bgOffsetX ?? 0);
  let bgOy = $derived($sceneStore.bgOffsetY ?? 0);
  let hasFond = $derived(bgMedia.length > 0);
  let bgIsVideo = $derived(bgKind === "video");
  // Badge « en lecture dans OBS » (fond) : bgPaused === false = la vidéo de
  // fond joue réellement côté diffusion (:4321) — vignette dashboard figée.
  let bgLecture = $derived(bgIsVideo && ($sceneStore.bgPaused ?? true) === false);

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
  let bgMediaTransform = $derived(`translate(${bgOx}px, ${bgOy}px) rotate(${bgRot}deg) scale(${bgZoom})`);

  // ===== Cadre de l'application (bord extérieur du canvas) =====
  let cadreApp = $derived($sceneStore.cadreApp ?? null);
  let cadreAppActif = $derived(!!cadreApp?.actif && (cadreApp?.strokeWidth ?? 0) > 0);
  // Couleur du badge « en lecture dans OBS » (fond) = couleur RÉELLE du
  // cadre app choisi (couleur utilisateur si personnalisable, palette de la
  // variante pour les presets). Fallback --texte si pas de cadre actif.
  let bgLectureCouleur = $derived(
    cadreAppActif && cadreApp ? couleurCadre(cadreApp.style, cadreApp.variante, cadreApp.couleur) : "var(--texte)"
  );

  // ===== Titre du fond de l'application (intérieur haut/bas) =====
  // Titre optionnel : texte + police Google Font + taille + position.
  // Le gradient du texte reprend les couleurs du cadre de l'application
  // (cadreApp) actif (fallback blanc→gris si aucun cadre actif).
  let bgTitreTexte = $derived($sceneStore.bgTitre ?? "");
  let bgTitreVisible = $derived(bgTitreTexte.length > 0);
  let bgTitrePos = $derived($sceneStore.bgTitrePosition ?? "haut");
  let bgTitrePolice = $derived($sceneStore.bgTitrePolice ?? POLICE_TITRE_DEFAUT);
  let bgTitreTaille = $derived($sceneStore.bgTitreTaille ?? TAILLE_TITRE_DEFAUT);
  let bgTitreGras = $derived($sceneStore.bgTitreGras ?? false);
  let bgTitreItalique = $derived($sceneStore.bgTitreItalique ?? false);
  let bgTitreSouligne = $derived($sceneStore.bgTitreSouligne ?? false);
  let bgTitreEspacement = $derived($sceneStore.bgTitreEspacement ?? ESPACEMENT_TITRE_DEFAUT);
  let bgTitreGradient = $derived(gradientCadre(cadreAppActif ? cadreApp : null));
  let bgTitreFontStyle = $derived(bgTitreItalique ? "italic" : "normal");
  let bgTitreFontWeight = $derived(bgTitreGras ? "700" : "400");
  let bgTitreTextDecoration = $derived(bgTitreSouligne ? "underline" : "none");

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

  // Clic canvas vide :
  //   - Mode morphing + fond IMAGE présent → bulge/pinch sur le fond au point
  //     cliqué (coords normalisées du canvas — indépendantes du scale).
  //   - Sinon → désélection + ouvre section Arrière-plan de l'application.
  function onCanvasPointerDown(e: PointerEvent) {
    if ($morphMode && hasFond && !bgIsVideo) {
      const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
      const nx = Math.min(1, Math.max(0, (e.clientX - rect.left) / rect.width));
      const ny = Math.min(1, Math.max(0, (e.clientY - rect.top) / rect.height));
      void ajouterMorphFond(nx, ny, $morphRayon, $morphIntensite, $morphSens);
      setCarteEdition("fond");
      return;
    }
    selectWidget(null);
    setCarteEdition("fond");
  }

  // Curseur cercle du fond (feedback rayon, comme MorphLens) — suivi pointer.
  // Coords locales du canvas (canvas px) — le parent est scalé → on compense.
  let bgMorphCx = $state(0);
  let bgMorphCy = $state(0);
  let bgMorphDiam = $state(0);
  let bgMorphSurvol = $state(false);

  function onCanvasPointerMove(e: PointerEvent) {
    if ($morphMode && hasFond && !bgIsVideo) {
      const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
      bgMorphCx = (e.clientX - rect.left) / scale;
      bgMorphCy = (e.clientY - rect.top) / scale;
      bgMorphDiam = $morphRayon * 2 * canvasW;
      bgMorphSurvol = true;
    }
  }

  function onCanvasPointerLeave() {
    bgMorphSurvol = false;
  }

  // ===== Morphing du fond (rendu canvas WebGL si bgMorphs) =====
  // Même pattern que les widgets : média → offscreen (fit + transforms fond) →
  // warp GL → canvas visible. Fond dashboard = vignette figée → one-shot.
  let bgAMorphs = $derived(($sceneStore.bgMorphs?.length ?? 0) > 0 && hasFond);
  let bgMorphCanvasEl: HTMLCanvasElement | undefined = $state(undefined);
  let bgImgEl: HTMLImageElement | undefined = $state(undefined);
  let bgRendu: RenduMorph | undefined = $state(undefined);
  let bgOff: HTMLCanvasElement | undefined = $state(undefined);
  let bgPret = $state(0);

  function rendreBgMorph(): void {
    if (!bgRendu) return;
    const src: HTMLImageElement | HTMLVideoElement | null = bgIsVideo
      ? bgVideoEl ?? null
      : bgImgEl ?? null;
    if (!src) return;
    const sw = bgIsVideo ? bgVideoEl!.videoWidth : bgImgEl!.naturalWidth;
    const sh = bgIsVideo ? bgVideoEl!.videoHeight : bgImgEl!.naturalHeight;
    if (!sw || !sh) return;
    if (!bgOff) bgOff = document.createElement("canvas");
    if (bgOff.width !== canvasW) bgOff.width = canvasW;
    if (bgOff.height !== canvasH) bgOff.height = canvasH;
    const ctx = bgOff.getContext("2d");
    if (!ctx) return;
    ctx.clearRect(0, 0, canvasW, canvasH);
    ctx.save();
    ctx.translate(canvasW / 2, canvasH / 2);
    ctx.translate(bgOx, bgOy);
    ctx.rotate((bgRot * Math.PI) / 180);
    ctx.scale(bgZoom, bgZoom);
    ctx.translate(-canvasW / 2, -canvasH / 2);
    dessinerFitCanvas(ctx, src, sw, sh, canvasW, canvasH, bgFitCss);
    ctx.restore();
    bgRendu.rendre(bgOff, $sceneStore.bgMorphs ?? []);
  }

  // Rendu one-shot réactif : morphs, média, dims, transforms fond, prêt.
  $effect(() => {
    if (!bgAMorphs || !bgRendu) return;
    void bgPret;
    void $sceneStore.bgMorphs;
    void bgMedia;
    canvasW;
    canvasH;
    void bgZoom;
    void bgRot;
    void bgOx;
    void bgOy;
    void bgFitCss;
    rendreBgMorph();
  });

  // Lifecycle GL (créé/détruit selon bgAMorphs).
  $effect(() => {
    if (bgAMorphs && bgMorphCanvasEl && !bgRendu) {
      try {
        bgRendu = creerRenduMorph(bgMorphCanvasEl);
      } catch {
      }
    }
    if (!bgAMorphs && bgRendu) {
      bgRendu.detruire();
      bgRendu = undefined;
    }
  });

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
      class:morphable={$morphMode && hasFond && !bgIsVideo}
      style="width:{canvasW}px; height:{canvasH}px; transform: scale({scale});"
      onpointerdown={onCanvasPointerDown}
      onpointermove={onCanvasPointerMove}
      onpointerleave={onCanvasPointerLeave}
      role="presentation"
    >
      {#if hasFond}
        <div class="bg-layer">
          {#if bgAMorphs}
            <!-- Rendu morphé : canvas GL (le warp inclut fit + transforms fond) -->
            <canvas bind:this={bgMorphCanvasEl} class="bg-media morph-canvas" draggable="false"></canvas>
            {#if bgIsVideo}
              <video
                bind:this={bgVideoEl}
                class="bg-src-cachee"
                muted
                playsinline
                preload="auto"
                crossorigin="anonymous"
                draggable="false"
                onloadeddata={() => (bgPret += 1)}
              ></video>
            {:else}
              <img
                bind:this={bgImgEl}
                class="bg-src-cachee"
                src={MEDIA_BASE + bgMedia}
                alt=""
                draggable="false"
                crossorigin="anonymous"
                onload={() => (bgPret += 1)}
              />
            {/if}
          {:else if bgIsVideo}
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
          <!-- Curseur cercle rayon : visible dès l'activation du mode (pas
               seulement après le premier morph) pour viser avant de cliquer. -->
          {#if $morphMode && bgMorphSurvol && !bgIsVideo}
            <div
              class="morph-curseur"
              style="left:{bgMorphCx}px; top:{bgMorphCy}px; width:{bgMorphDiam}px; height:{bgMorphDiam}px;"
            ></div>
          {/if}
          {#if bgLecture}
            <!-- Badge lecture OBS (dashboard seulement) : la barre lecteur
                 pilote :4321 via bgPaused — la vignette dashboard reste figée. -->
            <div class="lecture-obs-fond" style="color: {bgLectureCouleur};">
              <span class="lecture-obs-icon">▶</span>
              <span>Votre vidéo de fond est en lecture dans OBS</span>
            </div>
          {/if}
        </div>
      {/if}

      <!-- Titre du fond de l'application (intérieur haut/bas) — hors .bg-layer
           pour ne pas être clippé par overflow:hidden. Au-dessus du fond
           (z-index 1), sous les widgets. Gradient de texte reprenant les
           couleurs du cadre de l'application (cadreApp). -->
      {#if bgTitreVisible}
        <div
          class="bg-titre {bgTitrePos}"
          style="font-family: '{bgTitrePolice}', sans-serif; font-size: {bgTitreTaille}px; font-style: {bgTitreFontStyle}; font-weight: {bgTitreFontWeight}; text-decoration: {bgTitreTextDecoration}; letter-spacing: {bgTitreEspacement}px; background: linear-gradient(135deg, {bgTitreGradient.from}, {bgTitreGradient.to}); -webkit-background-clip: text; background-clip: text; color: transparent;"
        >{bgTitreTexte}</div>
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
            gradientAngle={cadreApp.gradientAngle}
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
    /* Outline (pas border) : dessiné HORS du box, ne consomme aucun pixel du
       canvas. Avec border + box-sizing:border-box global, la zone interne
       faisait canvasW-2 × canvasH-2 → décalage 1px + marges asymétriques
       vs OBS. Outline garantit un espace de coordonnées = canvasW×canvasH
       au pixel près, identique à la source SOS-Diffusion. */
    outline: 1px solid var(--bordure-active);
    flex-shrink: 0;
  }
  .bg-layer {
    position: absolute;
    inset: 0;
    overflow: hidden;
    z-index: 0;
    pointer-events: none;
  }
  /* ===== Titre du fond de l'application ===== */
  /* Positionné à l'intérieur du canvas, en haut ou en bas, centré
     horizontalement. Au-dessus du fond (z-index 1), sous les widgets.
     pointer-events:none pour ne pas bloquer le drag/sélection. */
  .bg-titre {
    position: absolute;
    left: 50%;
    transform: translateX(-50%);
    white-space: nowrap;
    pointer-events: none;
    z-index: 1;
    line-height: 1.1;
    font-weight: 400;
    text-align: center;
  }
  .bg-titre.haut { top: 8px; }
  .bg-titre.bas { bottom: 8px; }
  .bg-media {
    width: 100%;
    height: 100%;
  }
  /* Rendu morphé du fond + source cachée (même pattern que Widget.svelte). */
  .morph-canvas {
    display: block;
  }
  .bg-src-cachee {
    position: absolute;
    width: 1px;
    height: 1px;
    opacity: 0;
    pointer-events: none;
  }
  /* Mode morphing sur le fond : curseur croix + cercle rayon (orange
     actionnable). Le cercle est en coords LOCALES du canvas (canvas px) —
     le parent est scalé à l'affichage. */
  .canvas.morphable {
    cursor: crosshair;
  }
  .morph-curseur {
    position: absolute;
    border: 2px dashed var(--message-user-action-color);
    border-radius: 50%;
    pointer-events: none;
    transform: translate(-50%, -50%);
    z-index: 50;
  }
  .bg-video {
    pointer-events: none;
  }
  /* Badge « en lecture dans OBS » (fond) : pastille centrée sur le canvas,
     par-dessus la vignette figée. Variables CSS UI (pas de #hex). */
  .lecture-obs-fond {
    position: absolute;
    left: 50%;
    top: 50%;
    transform: translate(-50%, -50%);
    display: flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.45rem 0.7rem;
    background: var(--fond);
    background: color-mix(in srgb, var(--fond) 82%, transparent);
    font-size: 1.25rem;
    border-radius: var(--rayon-petit);
    white-space: nowrap;
    max-width: 96%;
    overflow: hidden;
    pointer-events: none;
    z-index: 2;
  }
  .lecture-obs-icon {
    font-size: 1.35rem;
    line-height: 1;
  }
  /* Overlay cadre app : bord extérieur du canvas, au-dessus des widgets. */
  .cadre-app-overlay {
    position: absolute;
    inset: 0;
    pointer-events: none;
    z-index: 100;
  }
</style>
