<script lang="ts">
  import {
    moveWidgetLocal,
    resizeWidgetLocal,
    commitScene,
    selectedIdStore,
    selectWidget,
    buildSnapTargets,
    computeSnap,
    sceneStore,
    type SnapTarget,
  } from "../stores/scene";
  import { afficherGuidesDrag, masquerGuides } from "../stores/guides";
  import { setCarteEdition } from "../stores/ui";
  import { registerVideo, unregisterVideo } from "../stores/video";
  import { morphMode, morphIntensite, morphRayon, morphSens } from "../stores/morph";
  import { ajouterMorphWidget } from "../stores/scene";
  import { creerRenduMorph, dessinerFitCanvas, chaineFilter, type RenduMorph } from "../morph/gl-morph";
  import type { Widget } from "../tauri";
  import { get } from "svelte/store";
  import CadreSVG from "./CadreSVG.svelte";
  import { genererClipPathCadre, couleurCadre, gradientCadre } from "../cadres/registre";
  import { titreModalOpen } from "../stores/ui";
  import { POLICE_TITRE_DEFAUT, TAILLE_TITRE_DEFAUT, ESPACEMENT_TITRE_DEFAUT } from "../fonts";
  import WidgetSpeedrun from "./WidgetSpeedrun.svelte";

  let { w, scale }: { w: Widget; scale: number } = $props();

  // Type du widget : "media" (défaut), "chat", "camera", "welcome-clip" ou
  // "input-viewer".
  let widgetType = $derived(w.type ?? "media");
  let isChat = $derived(widgetType === "chat");
  let isCamera = $derived(widgetType === "camera");
  let isWelcomeClip = $derived(widgetType === "welcome-clip");
  let isInputViewer = $derived(widgetType === "input-viewer");
  let isSpeedrun = $derived(widgetType === "speedrun");

  // Mini widget — jamais en dessous.
  const MIN = 80;
  // Poignées resize : 4 coins + 4 bords.
  const HANDLES = ["nw", "n", "ne", "e", "se", "s", "sw", "w"] as const;

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

  // Grille magnétique : cibles figées au pointerdown (snapshot, pas de get() par frame).
  // Bypass si Alt enfoncé (free-drag sans aimantation).
  let snapTargets: { xs: SnapTarget[]; ys: SnapTarget[] } | null = null;

  // Préfixe URL pour les médias servis par le serveur :4321 (dashboard ≠ :4321).
  const MEDIA_BASE = "http://127.0.0.1:4321/";

  let selected = $derived($selectedIdStore === w.id);
  let rx = $derived(w.rotateX ?? 0);
  let ry = $derived(w.rotateY ?? 0);
  let fit = $derived(w.mediaFit ?? "ajuster");
  let zoom = $derived(w.mediaZoom ?? 1);
  let rot = $derived(w.mediaRot ?? 0);
  let ox = $derived(w.mediaOffsetX ?? 0);
  let oy = $derived(w.mediaOffsetY ?? 0);
  // Effets visuels (luminosité/contraste/teinte/flou). La pixelisation est
  // appliquée en canvas uniquement (pas d'équivalent CSS pur) — le rendu DOM
  // l'ignore (dashboard = vignette de config, pas de preview pixel parfait).
  let mLum = $derived(w.mediaLum ?? 0);
  let mContraste = $derived(w.mediaContraste ?? 0);
  let mTeinte = $derived(w.mediaTeinte ?? 0);
  let mFlou = $derived(w.mediaFlou ?? 0);
  let mPixel = $derived(w.mediaPixel ?? 0);
  let mediaFilter = $derived(chaineFilter(mLum, mContraste, mTeinte, mFlou));
  // Transform centré sur l'élément média (img/video). Le cadre widget et le
  // gizmo 3D ne bougent pas — overflow:hidden coupe le débordement.
  // translate en premier (gauche) = décalage dans l'espace écran (intuitif).
  let mediaTransform = $derived(`translate(${ox}px, ${oy}px) rotate(${rot}deg) scale(${zoom})`);

  // Kind : depuis le champ, sinon déduit de l'extension (widgets existants).
  const VIDEO_EXT = ["mp4", "webm"];
  function kindFromMedia(rel: string): string {
    const ext = rel.split(".").pop()?.toLowerCase() ?? "";
    return VIDEO_EXT.includes(ext) ? "video" : "image";
  }
  let kind = $derived(w.kind ?? (w.media ? kindFromMedia(w.media) : "image"));
  let isVideo = $derived(kind === "video");
  let isTrou = $derived(w.trou === true);
  // Badge « en lecture dans OBS » : mediaPaused === false = la vidéo joue
  // réellement côté diffusion (:4321) — le dashboard reste une vignette figée.
  // Widgets trou exclus : côté diffusion ils sont transparents (pas de lecture).
  let lectureObs = $derived(isVideo && !!w.media && !isTrou && w.mediaPaused === false);

  // ===== Cadre de scène (overlay SVG + clip-path) =====
  // cadreWidget est lu depuis le store scène (un cadre pour tous les widgets).
  // Appliqué sur tous les widgets, y compris les widgets trou (le cadre SVG
  // encadre le trou, visible côté diffusion/OBS).
  // Exception : w.sansCadre (widgets créés par drop bibliothèque) — opt-out
  // par widget, le cadre de scène reste actif pour les autres.
  let cadre = $derived($sceneStore.cadreWidget ?? null);
  let cadreActif = $derived(!!cadre?.actif && (cadre?.strokeWidth ?? 0) > 0 && w.sansCadre !== true);
  // Couleur du badge « en lecture dans OBS » = couleur RÉELLE du cadre SVG
  // widget choisi (couleur utilisateur si personnalisable, palette de la
  // variante pour les presets). Fallback --texte si pas de cadre actif.
  let lectureCouleur = $derived(
    cadreActif && cadre ? couleurCadre(cadre.style, cadre.variante, cadre.couleur) : "var(--texte)"
  );
  let clipPathCorps = $derived(
    cadreActif && cadre
      ? genererClipPathCadre(cadre.style, cadre.variante, w.largeur, w.hauteur, cadre.strokeWidth) ?? ""
      : ""
  );

  // ===== Titre du widget (au-dessus ou en dessous) =====
  // Titre optionnel : texte + police Google Font + taille + position.
  // Le gradient du texte reprend les couleurs du cadre SVG actif de la scène
  // (fallback blanc→gris si aucun cadre actif).
  let titreTexte = $derived(w.titre ?? "");
  let titreVisible = $derived(titreTexte.length > 0);
  let titrePos = $derived(w.titrePosition ?? "dessus");
  let titrePolice = $derived(w.titrePolice ?? POLICE_TITRE_DEFAUT);
  let titreTaille = $derived(w.titreTaille ?? TAILLE_TITRE_DEFAUT);
  let titreGras = $derived(w.titreGras ?? false);
  let titreItalique = $derived(w.titreItalique ?? false);
  let titreSouligne = $derived(w.titreSouligne ?? false);
  let titreEspacement = $derived(w.titreEspacement ?? ESPACEMENT_TITRE_DEFAUT);
  let titreGradient = $derived(gradientCadre(cadreActif ? cadre : null));
  let titreFontStyle = $derived(titreItalique ? "italic" : "normal");
  let titreFontWeight = $derived(titreGras ? "700" : "400");
  let titreTextDecoration = $derived(titreSouligne ? "underline" : "none");

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

  // ===== Widget chat (dashboard) =====
  // RÈGLE D'OR : le dashboard est une CONFIG, pas un player.
  // Aucune bulle, aucun scroll, aucun avatar. Juste « Chat actif » centré.
  // Les bulles + avatar + badges s'affichent dans diffusion.html (OBS :4321).

  // ===== Morphing (rendu canvas WebGL si morphs) =====
  // Sans morph → rendu natif (img/video), zéro coût. Avec morphs → le média
  // est dessiné dans un offscreen 2D (fit + transforms existants), puis warpé
  // par le shader GL (chaîne de bulge/pinch) vers le canvas visible.
  // Dashboard = vignette figée → rendu one-shot réactif (pas de rAF).
  let aMorphs = $derived((w.morphs?.length ?? 0) > 0 && !!w.media);
  let morphCanvasEl: HTMLCanvasElement | undefined = $state(undefined);
  let imgEl: HTMLImageElement | undefined = $state(undefined);
  let renduMorph: RenduMorph | undefined = $state(undefined);
  let offscreen: HTMLCanvasElement | undefined = $state(undefined);
  let mediaPret = $state(0); // tick : incrémenté au load (img) / loadeddata (vidéo)

  // Curseur cercle (feedback rayon sous le pointeur, comme MorphLens).
  // Position/diamètre en coords LOCALES du widget (canvas px) — le parent est
  // scalé par `scale` → on compense pour que le cercle soit sous le pointeur.
  let morphSurvol = $state(false);
  let morphCx = $state(0);
  let morphCy = $state(0);
  let morphDiam = $state(0);
  let morphable = $derived(
    $morphMode && w.type === "media" && !!w.media && kind !== "video"
  );

  $effect(() => {
    if (aMorphs && morphCanvasEl && !renduMorph) {
      try {
        renduMorph = creerRenduMorph(morphCanvasEl);
      } catch {
      }
    }
    if (!aMorphs && renduMorph) {
      renduMorph.detruire();
      renduMorph = undefined;
    }
  });

  /// object-fit supprimé — partagé dans gl-morph.ts (dessinerFitCanvas).
  /// Dessine le média (img/video) avec fit + transforms média dans l'offscreen.
  /// Applique les effets visuels (filter CSS = brightness/contrast/hue/blur)
  /// + pixelisation (downscale→upscale avec imageSmoothingEnabled=false).
  function dessinerOffscreen(): HTMLCanvasElement | null {
    const src: HTMLImageElement | HTMLVideoElement | null =
      isVideo ? videoEl ?? null : imgEl ?? null;
    if (!src) return null;
    const sw = isVideo ? (src as HTMLVideoElement).videoWidth : (src as HTMLImageElement).naturalWidth;
    const sh = isVideo ? (src as HTMLVideoElement).videoHeight : (src as HTMLImageElement).naturalHeight;
    if (!sw || !sh) return null;
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    const cw = Math.max(1, Math.round(w.largeur * dpr));
    const ch = Math.max(1, Math.round(w.hauteur * dpr));
    if (!offscreen) offscreen = document.createElement("canvas");
    if (offscreen.width !== cw) offscreen.width = cw;
    if (offscreen.height !== ch) offscreen.height = ch;
    const ctx = offscreen.getContext("2d");
    if (!ctx) return null;
    ctx.clearRect(0, 0, cw, ch);
    // Effets CSS filter (brightness/contrast/hue-rotate/blur). Vide = neutre.
    ctx.filter = mediaFilter;
    ctx.save();
    // Même ordre que le transform CSS : translate(offset) rotate scale, origin centre.
    ctx.translate(cw / 2, ch / 2);
    ctx.translate(ox * dpr, oy * dpr);
    ctx.rotate((rot * Math.PI) / 180);
    ctx.scale(zoom, zoom);
    ctx.translate(-cw / 2, -ch / 2);
    // Pixelisation : si mediaPixel > 0, on dessine dans un petit canvas
    // intermédiaire puis on l'upscale sans lissage. Le facteur de pixelisation
    // réduit la résolution (cw/pixel) → blocs visibles.
    if (mPixel > 0) {
      const pf = Math.max(1, mPixel);
      const pw = Math.max(1, Math.round(cw / pf));
      const ph = Math.max(1, Math.round(ch / pf));
      const pix = document.createElement("canvas");
      pix.width = pw;
      pix.height = ph;
      const pctx = pix.getContext("2d");
      if (pctx) {
        pctx.filter = mediaFilter;
        pctx.save();
        pctx.translate(pw / 2, ph / 2);
        pctx.translate((ox * dpr) / pf, (oy * dpr) / pf);
        pctx.rotate((rot * Math.PI) / 180);
        pctx.scale(zoom, zoom);
        pctx.translate(-pw / 2, -ph / 2);
        dessinerFitCanvas(pctx, src, sw, sh, pw, ph, fitCss);
        pctx.restore();
        ctx.filter = "none";
        ctx.imageSmoothingEnabled = false;
        ctx.drawImage(pix, 0, 0, pw, ph, 0, 0, cw, ch);
        ctx.imageSmoothingEnabled = true;
        ctx.restore();
        return offscreen;
      }
    }
    dessinerFitCanvas(ctx, src, sw, sh, cw, ch, fitCss);
    ctx.restore();
    ctx.filter = "none";
    return offscreen;
  }

  function rendreMorph(): void {
    if (!renduMorph) return;
    const src = dessinerOffscreen();
    if (!src) return;
    renduMorph.rendre(src, w.morphs ?? []);
  }

  // Rendu one-shot réactif : morphs, média, dims, transforms, fit, prêt,
  // mediaTime (seek barre lecteur → la vignette warpée se resynchronise).
  $effect(() => {
    if (!aMorphs || !renduMorph) return;
    void mediaPret;
    void w.morphs;
    void w.media;
    void w.mediaTime;
    w.largeur;
    w.hauteur;
    void zoom;
    void rot;
    void ox;
    void oy;
    void fitCss;
    void mLum;
    void mContraste;
    void mTeinte;
    void mFlou;
    void mPixel;
    rendreMorph();
  });

  function onpointerdown(e: PointerEvent) {
    e.stopPropagation();
    selectWidget(w.id);
    // Mode morphing actif + widget média IMAGE → clic = bulge/pinch au point
    // cliqué (pas de drag). Coordonnées normalisées du widget.
    if (morphable) {
      const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
      const nx = Math.min(1, Math.max(0, (e.clientX - rect.left) / rect.width));
      const ny = Math.min(1, Math.max(0, (e.clientY - rect.top) / rect.height));
      void ajouterMorphWidget(w.id, nx, ny, $morphRayon, $morphIntensite, $morphSens);
      return;
    }
    setCarteEdition("widget");
    dragging = true;
    startX = e.clientX;
    startY = e.clientY;
    startWX = w.x;
    startWY = w.y;
    startW = w.largeur;
    startH = w.hauteur;
    // Snapshot des cibles de snap (figé pendant tout le drag — pas de get() par frame).
    snapTargets = buildSnapTargets(get(sceneStore), w.id);
    // Afficher les guides d'alignement (dashboard uniquement).
    afficherGuidesDrag({ x: w.x, y: w.y, width: w.largeur, height: w.hauteur });
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }

  function onHandleDown(e: PointerEvent, h: string) {
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
    // Capture sur l'outer : move/up écoutés sur l'outer.
    outerEl.setPointerCapture(e.pointerId);
  }

  function onpointermove(e: PointerEvent) {
    // Curseur morphing : cercle rayon sous le pointeur (feedback MorphLens).
    // Coords locales = écran - rect, divisées par le scale d'affichage.
    if (morphable) {
      const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
      morphCx = (e.clientX - rect.left) / scale;
      morphCy = (e.clientY - rect.top) / scale;
      morphDiam = $morphRayon * 2 * w.largeur;
      morphSurvol = true;
    }
    if (dragging) {
      const rawX = startWX + (e.clientX - startX) / scale;
      const rawY = startWY + (e.clientY - startY) / scale;
      // Grille magnétique sauf si Alt enfoncé (free-drag).
      let nx = rawX;
      let ny = rawY;
      if (!e.altKey && snapTargets) {
        const snap = computeSnap(rawX, rawY, w.largeur, w.hauteur, snapTargets);
        nx = snap.x;
        ny = snap.y;
      }
      moveWidgetLocal(w.id, nx, ny);
      // Mettre à jour les guides visuels pendant le drag.
      afficherGuidesDrag({ x: nx, y: ny, width: w.largeur, height: w.hauteur });
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
  }

  async function onpointerup(e: PointerEvent) {
    if (!dragging && !resizing) return;
    dragging = false;
    resizing = false;
    handle = "";
    snapTargets = null;
    masquerGuides();
    (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId);
    await commitScene();
  }

  // Double-clic sur le haut ou le bas du widget → ouvre la modale titre.
  // La zone haut (25% supérieur) présélectionne "dessus", le reste "dessous".
  function ondblclick(e: MouseEvent) {
    e.stopPropagation();
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const relY = (e.clientY - rect.top) / rect.height;
    const position = relY < 0.25 ? "dessus" : "dessous";
    // Pré-régler la position si le widget n'a pas encore de titre.
    if (!titreVisible) {
      sceneStore.update((s) => ({
        ...s,
        widgets: s.widgets.map((w2) =>
          w2.id === w.id ? { ...w2, titrePosition: position } : w2
        ),
      }));
    }
    titreModalOpen.set(w.id);
  }
</script>

<!-- Outer = hit-box rectangle NON incliné (drag position + poignées resize). -->
<div
  class="widget"
  class:dragging
  class:morphable
  bind:this={outerEl}
  style="left:{w.x}px; top:{w.y}px; width:{w.largeur}px; height:{w.hauteur}px; z-index:{w.z};"
  onpointerdown={onpointerdown}
  onpointermove={onpointermove}
  onpointerup={onpointerup}
  onpointerleave={() => (morphSurvol = false)}
  ondblclick={ondblclick}
  role="button"
  tabindex="0"
>
  <!-- Snippet : média de fond des widgets à contenu (chat / input-viewer /
       speedrun). Aperçu dashboard WYSIWYG : img figée ou vidéo vignette
       (mêmes $effect que les widgets média — src, registerVideo, mediaTime).
       Rendu en premier enfant + .media-fond (absolute z-index 0) → le
       contenu peint par-dessus. Côté diffusion, syncMediaFond
       (diffusion.html) applique le même empilement. -->
  {#snippet mediaFond()}
    {#if w.media}
      {#if isVideo}
        <video
          class="media-fond"
          bind:this={videoEl}
          muted
          playsinline
          preload="metadata"
          draggable="false"
          style="object-fit:{fitCss}; object-position:{fit === 'centrer' ? 'center' : '50% 50%'}; transform:{mediaTransform}; transform-origin:center center; filter:{mediaFilter};"
        ></video>
      {:else}
        <img
          class="media-fond"
          src={MEDIA_BASE + w.media}
          alt=""
          draggable="false"
          style="object-fit:{fitCss}; object-position:{fit === 'centrer' ? 'center' : '50% 50%'}; transform:{mediaTransform}; transform-origin:center center; filter:{mediaFilter};"
        />
      {/if}
    {/if}
  {/snippet}

  <!-- Inner = bloc visuel incliné (cadre + image). pointer-events:none.
       Quand un cadre de scène est actif : clip-path découpe le contenu selon
       la forme du cadre (coins biseautés, hexagone, etc.) et la bordure native
       .widget-3d est masquée (le cadre SVG remplace). -->
  <div
    class="widget-3d"
    class:selected
    class:avec-cadre={cadreActif}
    class:input-viewer={isInputViewer}
    class:speedrun={isSpeedrun}
    class:sans-cadre={w.sansCadre === true}
    style="transform: perspective(800px) rotateX({rx}deg) rotateY({ry}deg);{clipPathCorps ? ` clip-path: ${clipPathCorps}; -webkit-clip-path: ${clipPathCorps};` : ''}"
  >
    {#if isChat}
      <!-- Widget chat (dashboard) : « Chat actif » centré seulement.
           Aucune bulle, aucun scroll, aucun avatar. Les bulles s'affichent
           dans diffusion.html (OBS :4321) via WS {"type":"chat"}. -->
      {@render mediaFond()}
      <div class="chat-actif">Chat actif</div>
    {:else if isCamera}
      <!-- Widget caméra (dashboard) : placeholder seulement. L'image vient de
           la source OBS "SOS-Caméra" (dshow_input sous SOS-Diffusion) qui
           apparaît à travers le trou du widget dans diffusion.html. Le
           dashboard est une config, pas un player. -->
      <div class="camera-actif">
        <span>Caméra</span>
      </div>
    {:else if isWelcomeClip}
      <!-- Widget clip de bienvenue (dashboard) : placeholder seulement.
           Le clip se joue dans diffusion.html (OBS :4321) via WS
           {"type":"welcome-clip-play"}. Position/taille/z pilotées ici. -->
      <div class="welcome-clip-actif">
        <span>Clip de bienvenue</span>
      </div>
    {:else if isInputViewer}
      <!-- Widget input viewer (dashboard) : badge "affiché dans OBS" seulement.
           Les entrées (clavier/souris/numpad/manette) s'affichent dans
           diffusion.html (OBS :4321) via WS {"type":"input-viewer-etat"}.
           Mode/layout/skin/couleur réglés dans la Toolbar ; capture ON/OFF
           dans les options. -->
      {@render mediaFond()}
      <div class="input-viewer-obs msg-pill">
        <span class="msg-pill-icon">▶</span>
        <span class="msg-pill-text">Votre input viewer est affiché dans OBS</span>
      </div>
    {:else if isSpeedrun}
      <!-- Widget speedrun splitter (dashboard) : timer + contrôles manuels +
           segments LSS + settings ASL. Le rendu diffusion est dans
           diffusion.html (OBS :4321) via WS {"type":"speedrun-etat"}. -->
      {@render mediaFond()}
      <div class="contenu-dessus">
        <WidgetSpeedrun contenu={w} gradient={titreGradient} />
      </div>
    {:else if w.media && !isVideo}
      {#if aMorphs}
        <!-- Rendu morphé : canvas GL (le warp inclut fit + transforms) -->
        <canvas bind:this={morphCanvasEl} class="preview morph-canvas" draggable="false"></canvas>
        <img
          bind:this={imgEl}
          class="src-cachee"
          src={MEDIA_BASE + w.media}
          alt=""
          draggable="false"
          crossorigin="anonymous"
          onload={() => (mediaPret += 1)}
        />
      {:else}
        <img
          class="preview"
          src={MEDIA_BASE + w.media}
          alt=""
          draggable="false"
          style="object-fit:{fitCss}; object-position:{fit === 'centrer' ? 'center' : '50% 50%'}; transform:{mediaTransform}; transform-origin:center center; filter:{mediaFilter};"
        />
      {/if}
    {:else if w.media && isVideo}
      {#if aMorphs}
        <!-- Rendu morphé : canvas GL, la vidéo reste la source (cachée) -->
        <canvas bind:this={morphCanvasEl} class="preview morph-canvas" draggable="false"></canvas>
        <video
          bind:this={videoEl}
          class="src-cachee"
          muted
          playsinline
          preload="auto"
          crossorigin="anonymous"
          draggable="false"
          onloadeddata={() => (mediaPret += 1)}
        ></video>
      {:else}
        <video
          class="preview video"
          bind:this={videoEl}
          muted
          playsinline
          preload="metadata"
          draggable="false"
          style="object-fit:{fitCss}; object-position:{fit === 'centrer' ? 'center' : '50% 50%'}; transform:{mediaTransform}; transform-origin:center center; filter:{mediaFilter};"
        ></video>
      {/if}
    {/if}

    {#if lectureObs}
      <!-- Badge lecture OBS (dashboard seulement) : la barre lecteur pilote
           :4321 via mediaPaused — la vignette dashboard reste figée. -->
      <div class="lecture-obs msg-pill" style="color: {lectureCouleur};">
        <span class="msg-pill-icon">▶</span>
        <span class="msg-pill-text">Votre vidéo est en lecture dans OBS</span>
      </div>
    {/if}

    <!-- Cadre de scène (overlay SVG) — visible en dashboard ET en diffusion.
         Ne capture pas les événements (pointer-events: none) pour ne pas
         bloquer le drag/sélection. Appliqué sur tous les widgets y compris
         les widgets trou (le cadre encadre le trou). -->
    {#if cadreActif && cadre}
      <div class="widget-cadre">
        <CadreSVG
          style={cadre.style}
          variante={cadre.variante}
          largeur={w.largeur}
          hauteur={w.hauteur}
          strokeWidth={cadre.strokeWidth}
          couleur={cadre.couleur}
          couleurFin={cadre.couleurFin}
          gradientAngle={cadre.gradientAngle}
          idCadre="w-{w.id}"
        />
      </div>
    {/if}
  </div>

  <!-- Titre du widget (au-dessus ou en dessous) — hors .widget-3d pour ne pas
       être clippé par le cadre SVG. Gradient de texte reprenant les couleurs
       du cadre actif (fallback blanc→gris). Police Google Font + taille px. -->
  {#if titreVisible}
    <div
      class="widget-titre {titrePos}"
      style="font-family: '{titrePolice}', sans-serif; font-size: {titreTaille}px; font-style: {titreFontStyle}; font-weight: {titreFontWeight}; text-decoration: {titreTextDecoration}; letter-spacing: {titreEspacement}px; background: linear-gradient(135deg, {titreGradient.from}, {titreGradient.to}); -webkit-background-clip: text; background-clip: text; color: transparent;"
    >{titreTexte}</div>
  {/if}

  <!-- Zones de double-clic (haut/bas) — affichées au survol du widget
       sélectionné (avec ou sans titre existant) pour indiquer où
       double-cliquer pour ajouter/modifier un titre. -->
  {#if selected}
    <div class="titre-zone titre-zone-haut">
      <div class="msg-pill">
        <span class="msg-pill-icon">▶</span>
        <span class="msg-pill-text"
          >Double-cliquez ici pour {titreVisible ? "modifier le" : "ajouter un"} titre au-dessus de votre widget</span
        >
      </div>
    </div>
    <div class="titre-zone titre-zone-bas">
      <div class="msg-pill">
        <span class="msg-pill-icon">▶</span>
        <span class="msg-pill-text"
          >Double-cliquez ici pour {titreVisible ? "modifier le" : "ajouter un"} titre en dessous de votre widget</span
        >
      </div>
    </div>
  {/if}

  <!-- Overlay trou : cadre pointillé par-dessus le widget quand trou activé.
       Le widget reste sélectionnable (l'outer hit-box est intact).
       Tilté avec la même perspective que .widget-3d → correspond au trou :4321. -->
  {#if isTrou && !cadreActif}
    <div
      class="trou-overlay"
      style="transform: perspective(800px) rotateX({rx}deg) rotateY({ry}deg);"
    ></div>
  {/if}

  <!-- Curseur morphing : cercle rayon sous le pointeur (feedback MorphLens).
       Visible seulement en mode morphing sur un média IMAGE morphable. -->
  {#if morphable && morphSurvol}
    <div
      class="morph-curseur"
      style="left:{morphCx}px; top:{morphCy}px; width:{morphDiam}px; height:{morphDiam}px;"
    ></div>
  {/if}

  <!-- Poignées resize : 4 coins + 4 bords. Widget sélectionné seulement.
       Affordance souris pure — le widget parent (role=button, tabindex=0)
       reste l'unique cible clavier, d'où le svelte-ignore. -->
  {#if selected}
    {#each HANDLES as h (h)}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="handle {h}" onpointerdown={(e) => onHandleDown(e, h)}></div>
    {/each}
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
    /* Conteneur de requête pour le font-size cqw des pastilles .msg-pill
       (recette app.css) — le texte suit la taille réelle du widget. */
    container-type: inline-size;
  }
  /* Quand un cadre SVG est actif : masquer la bordure native (le cadre SVG
     remplace). L'outline de sélection reste pour le feedback dashboard. */
  .widget-3d.avec-cadre {
    border-color: transparent;
  }
  /* Widgets sansCadre (drop bibliothèque) : aucun contour 1px — seul le
     média est visible. L'outline .selected (gizmo de sélection) reste. */
  .widget-3d.sans-cadre {
    border: none;
  }
  .widget-3d.selected {
    outline: 2px solid var(--texte);
    outline-offset: -2px;
  }
  /* Overlay cadre SVG : position absolute, inset 0, au-dessus du média,
     sous les poignées resize. pointer-events:none pour ne pas bloquer le drag. */
  .widget-cadre {
    position: absolute;
    inset: 0;
    pointer-events: none;
    z-index: 5;
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
  .chat-actif {
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    /* Au-dessus du média de fond (.media-fond z-index 0). */
    position: relative;
    z-index: 1;
    pointer-events: none;
    opacity: 0.6;
    font-size: 0.9rem;
  }
  /* Média de fond des widgets à contenu (chat / input-viewer / speedrun) :
     premier enfant, absolute z-index 0 → le contenu peint par-dessus
     (.chat-actif, .input-viewer-obs badge z 2, .contenu-dessus). */
  .media-fond {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: fill;
    display: block;
    pointer-events: none;
    transform-origin: center center;
    z-index: 0;
  }
  /* Wrapper contenu au-dessus du média de fond (speedrun dashboard). */
  .contenu-dessus {
    position: relative;
    z-index: 1;
    width: 100%;
    height: 100%;
  }
  .camera-actif {
    width: 100%;
    height: 100%;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 0.4rem;
    pointer-events: none;
    opacity: 0.7;
    font-size: 0.9rem;
    background: #000;
    color: var(--texte);
  }
  /* Rendu morphé : canvas final (le warp inclut fit + transforms).
     Source cachée : hors flux mais "rendue" (1px, opacity 0) pour que la
     vidéo continue de décoder — display:none casserait les frames. */
  .morph-canvas {
    width: 100%;
    height: 100%;
    display: block;
  }
  .src-cachee {
    position: absolute;
    width: 1px;
    height: 1px;
    opacity: 0;
    pointer-events: none;
  }
  /* Mode morphing : curseur croix + cercle rayon sous le pointeur (orange
     actionnable — variable --message-user-action-color). */
  .widget.morphable {
    cursor: crosshair;
  }
  .morph-curseur {
    position: absolute;
    border: 2px dashed var(--message-user-action-color);
    border-radius: 50%;
    pointer-events: none;
    transform: translate(-50%, -50%);
    z-index: 20;
  }
  .welcome-clip-actif {
    width: 100%;
    height: 100%;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 0.4rem;
    pointer-events: none;
    opacity: 0.7;
    font-size: 0.9rem;
    background: #000;
    color: var(--texte);
  }
  /* Badge « input viewer affiché dans OBS » (dashboard) : pastille centrée
     .msg-pill (recette app.css), teinte orange action utilisateur. */
  .input-viewer-obs {
    color: var(--message-user-action-color);
  }
  /* Fond du widget input viewer (dashboard) : voile opaque sombre + halo
     orange doux au centre — signale visuellement l'overlay OBS. */
  .widget-3d.input-viewer {
    background:
      radial-gradient(ellipse at center,
        color-mix(in srgb, var(--message-user-action-color) 28%, transparent) 0%,
        color-mix(in srgb, var(--message-user-action-color) 10%, transparent) 40%,
        transparent 75%),
      rgba(12, 14, 22, 0.92);
    box-shadow: inset 0 0 2.5rem color-mix(in srgb, var(--message-user-action-color) 16%, transparent);
  }
  /* Badge « en lecture dans OBS » : pastille .msg-pill (recette app.css),
     couleur inline lectureCouleur = couleur réelle du cadre SVG. Dans
     .widget-3d → suit le tilt 3D et est découpé par le clip-path du cadre. */
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

  /* ===== Titre du widget ===== */
  /* Positionné au-dessus ou en dessous du widget, centré horizontalement.
     Hors .widget-3d → non clippé par le cadre SVG. pointer-events:none
     pour ne pas bloquer le drag/sélection. */
  .widget-titre {
    position: absolute;
    left: 50%;
    transform: translateX(-50%);
    white-space: nowrap;
    pointer-events: none;
    z-index: 10;
    line-height: 1.1;
    font-weight: 400;
    text-align: center;
    /* Contour blanc 2px autour des lettres : paint-order:stroke paint le
       contour SOUS le gradient de remplissage (sinon il mange les glyphes). */
    -webkit-text-stroke: 2px #fff;
    paint-order: stroke fill;
  }
  .widget-titre.dessus {
    bottom: 100%;
    margin-bottom: 2px;
  }
  .widget-titre.dessous {
    top: 100%;
    margin-top: 2px;
  }
  /* Zones de double-clic (haut/bas) — vraie pastille .msg-pill (même
     recette que « lecture OBS », teinte ambre action) au survol du widget
     sélectionné, avec ou sans titre existant. container-type:inline-size
     pour le cqw de la pastille. pointer-events:none (le dblclick est
     géré par le parent). */
  .titre-zone {
    position: absolute;
    left: 0;
    width: 100%;
    height: 25%;
    container-type: inline-size;
    pointer-events: none;
    z-index: 3;
    opacity: 0;
    transition: opacity 0.15s ease;
    background: color-mix(in srgb, var(--message-user-action-color) 8%, transparent);
    border: 1px dashed color-mix(in srgb, var(--message-user-action-color) 40%, transparent);
  }
  .titre-zone-haut { top: 0; }
  .titre-zone-bas { bottom: 0; }
  .widget:hover .titre-zone { opacity: 1; }
  .titre-zone .msg-pill {
    color: var(--message-user-action-color);
  }
</style>
