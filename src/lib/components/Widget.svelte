<script lang="ts">
  import {
    moveWidgetLocal,
    commitScene,
    selectedIdStore,
    selectWidget,
    buildSnapTargets,
    computeSnap,
    sceneStore,
    mettreWidgetPlan,
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
  import { genererClipPathCadre, gradientCadre } from "../cadres/registre";
  import { titreModalOpen, type PositionTitre } from "../stores/ui";
  import { POLICE_TITRE_DEFAUT, TAILLE_TITRE_DEFAUT, ESPACEMENT_TITRE_DEFAUT } from "../fonts";
  import { valeursGoals, goalsTwitch } from "../stores/objectifs";
  import { t, localeTag } from "../stores/langue";
  import WidgetSpeedrun from "./WidgetSpeedrun.svelte";
  import WidgetMusique from "./WidgetMusique.svelte";

  let { w, scale }: { w: Widget; scale: number } = $props();

  // Type du widget : "media" (défaut), "chat", "camera" ou "input-viewer".
  let widgetType = $derived(w.type ?? "media");
  let isChat = $derived(widgetType === "chat");
  let isCamera = $derived(widgetType === "camera");
  let isInputViewer = $derived(widgetType === "input-viewer");
  let isSpeedrun = $derived(widgetType === "speedrun");
  let isGoal = $derived(widgetType === "goal");
  let isMusique = $derived(widgetType === "musique");

  // ===== Widget musique (aperçu dashboard) =====
  // Vignette Winamp : rendu complet dans WidgetMusique.svelte (miroir 1:1
  // du widget musique de diffusion.html — marquee, temps, peaks, tick).

  // ===== Widget goal (aperçu dashboard) =====
  // Mode manuel : valeur live depuis valeursGoals (communaute/viewers).
  // Mode lié (goalId) : valeur du Creator Goal Twitch (frames goal-twitch).
  // Le rendu complet (progression + célébration) est dans diffusion.html —
  // ici c'est une vignette de config.
  let goalLieLive = $derived(w.goalId ? $goalsTwitch[w.goalId] : undefined);
  let goalActuel = $derived(
    w.goalId
      ? goalLieLive?.actuel
      : w.goalPlateforme && w.goalMetrique
        ? $valeursGoals[w.goalPlateforme]?.[w.goalMetrique]
        : undefined
  );
  let goalCibleLive = $derived(w.goalId ? goalLieLive?.cible : w.goalCible);
  let goalPct = $derived(
    goalActuel !== undefined && goalCibleLive
      ? Math.min(100, Math.round((goalActuel / goalCibleLive) * 100))
      : null
  );
  // Libellé en mode lié : description saisie par le streamer sur Twitch,
  // sinon label du type de goal (goal.ttype.*).
  let goalLabelLie = $derived.by(() => {
    if (!w.goalId) return null;
    if (!goalLieLive) return $t("carteEdition.goalsTwitch");
    return goalLieLive.description || $t("goal.ttype." + goalLieLive.goal_type);
  });

  // Le resize (poignées) est géré par SelectionOverlay.svelte — frère des
  // .widget dans Canvas, au-dessus de tout (jamais masqué par un voisin).

  let dragging = $state(false);
  let startX = 0;
  let startY = 0;
  let startWX = 0;
  let startWY = 0;

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
  // Miroir média : flip H/V du contenu — scale ±1 combiné au zoom, dans
  // l'espace local du média (après rotate dans la liste = flip de la source).
  let mMiroirH = $derived(w.mediaMiroirH === true);
  let mMiroirV = $derived(w.mediaMiroirV === true);
  let mediaFilter = $derived(chaineFilter(mLum, mContraste, mTeinte, mFlou));
  // Transform centré sur l'élément média (img/video). Le cadre widget et le
  // gizmo 3D ne bougent pas — overflow:hidden coupe le débordement.
  // translate en premier (gauche) = décalage dans l'espace écran (intuitif).
  let mediaTransform = $derived(`translate(${ox}px, ${oy}px) rotate(${rot}deg) scale(${zoom * (mMiroirH ? -1 : 1)}, ${zoom * (mMiroirV ? -1 : 1)})`);

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

  // ===== Carrousel d'images (mediaSlides) =====
  // Liste non vide = carrousel actif (prioritaire sur w.media, qui reste
  // stocké). Preview dashboard = slide courante avancée par un intervalle
  // LOCAL (le timer viewer est dans diffusion.html — aucune écriture scène
  // par slide). Swap de src simple, pas de fondu : vignette de config.
  let slidesList = $derived(w.mediaSlides && w.mediaSlides.length ? w.mediaSlides : null);
  let slideIdx = $state(0);
  let slideCourante = $derived(slidesList ? slidesList[Math.min(slideIdx, slidesList.length - 1)] : null);
  $effect(() => {
    // Liste changée (ajout/retrait/réordonnancement) → repartir au début.
    void slidesList;
    slideIdx = 0;
  });
  $effect(() => {
    if (!slidesList || slidesList.length < 2) return;
    const ms = Math.max(1, w.mediaSlidesSec ?? 5) * 1000;
    const timer = setInterval(() => {
      slideIdx = (slideIdx + 1) % slidesList.length;
    }, ms);
    return () => clearInterval(timer);
  });

  // ===== Cadre de scène (overlay SVG + clip-path) =====
  // cadreWidget est lu depuis le store scène (un cadre pour tous les widgets).
  // Appliqué sur tous les widgets, y compris les widgets trou (le cadre SVG
  // encadre le trou, visible côté diffusion/OBS).
  // Exception : w.sansCadre (widgets créés par drop bibliothèque) — opt-out
  // par widget, le cadre de scène reste actif pour les autres.
  let cadre = $derived($sceneStore.cadreWidget ?? null);
  let cadreActif = $derived(!!cadre?.actif && (cadre?.strokeWidth ?? 0) > 0 && w.sansCadre !== true);
  // Le badge « en lecture dans OBS » est orange fixe (.lecture-obs —
  // recette .msg-pill, app.css) — jamais la couleur du cadre.
  // Épaisseur effective : le widget goal adopte l'échelle « bulle »
  // (ratio 40% du stroke scène, plafond 8px — même cadre que les messages
  // chat en mode bulles en diffusion, cadreMsgScale/cadreGoal de
  // diffusion.html) ; son format compact rendrait le stroke scène
  // disproportionné.
  let cadreStroke = $derived(
    isGoal ? Math.max(1, Math.min(Math.round((cadre?.strokeWidth ?? 4) * 0.4), 8)) : (cadre?.strokeWidth ?? 4)
  );
  let clipPathCorps = $derived(
    cadreActif && cadre
      ? genererClipPathCadre(cadre.style, w.largeur, w.hauteur, cadreStroke) ?? ""
      : ""
  );

  // ===== Titres du widget (haut/bas/gauche/droite) =====
  // Jusqu'à 4 titres simultanés (un texte par position, champ vide = aucun),
  // style partagé (police/taille/…). Le gradient du texte reprend les
  // couleurs du cadre SVG actif de la scène (fallback blanc→gris si aucun
  // cadre actif). Latéraux (gauche/droite) : texte vertical (CSS
  // writing-mode ci-dessous).
  let titreHaut = $derived(w.titreHaut ?? "");
  let titreBas = $derived(w.titreBas ?? "");
  let titreGauche = $derived(w.titreGauche ?? "");
  let titreDroite = $derived(w.titreDroite ?? "");
  let titrePolice = $derived(w.titrePolice ?? POLICE_TITRE_DEFAUT);
  let titreTaille = $derived(w.titreTaille ?? TAILLE_TITRE_DEFAUT);
  let titreGras = $derived(w.titreGras ?? false);
  let titreItalique = $derived(w.titreItalique ?? false);
  let titreSouligne = $derived(w.titreSouligne ?? false);
  let titreEspacement = $derived(w.titreEspacement ?? ESPACEMENT_TITRE_DEFAUT);
  let titreContourCouleur = $derived(w.titreContourCouleur ?? "#fff");
  let titreContourEpaisseur = $derived(w.titreContourEpaisseur ?? 2);
  let cadreGradient = $derived(gradientCadre(cadreActif ? cadre : null));
  // Dégradé du texte : personnalisé si titreCouleur défini, sinon cadre.
  let titreGradient = $derived(
    w.titreCouleur ? { from: w.titreCouleur, to: w.titreCouleurFin ?? w.titreCouleur } : cadreGradient
  );
  let titreFontStyle = $derived(titreItalique ? "italic" : "normal");
  let titreFontWeight = $derived(titreGras ? "700" : "400");
  let titreTextDecoration = $derived(titreSouligne ? "underline" : "none");
  // Style inline partagé par les 4 titres (même config, textes différents).
  let titreStyle = $derived(
    "font-family: '" + titrePolice + "', sans-serif; font-size: " + titreTaille +
    "px; font-style: " + titreFontStyle + "; font-weight: " + titreFontWeight +
    "; text-decoration: " + titreTextDecoration + "; letter-spacing: " + titreEspacement +
    "px; -webkit-text-stroke: " + titreContourEpaisseur + "px " + titreContourCouleur +
    "; background: linear-gradient(135deg, " + titreGradient.from + ", " + titreGradient.to +
    "); -webkit-background-clip: text; background-clip: text; color: transparent;"
  );

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
    // Même ordre que le transform CSS : translate(offset) rotate scale(zoom*±1
    // miroir), origin centre.
    const mhs = mMiroirH ? -1 : 1;
    const mvs = mMiroirV ? -1 : 1;
    ctx.translate(cw / 2, ch / 2);
    ctx.translate(ox * dpr, oy * dpr);
    ctx.rotate((rot * Math.PI) / 180);
    ctx.scale(zoom * mhs, zoom * mvs);
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
        pctx.scale(zoom * mhs, zoom * mvs);
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
    void slideCourante; // carrousel : re-rendre quand la slide change
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
    void mMiroirH;
    void mMiroirV;
    rendreMorph();
  });

  function onpointerdown(e: PointerEvent) {
    // Bouton gauche uniquement : le clic droit ouvre le menu contextuel du
    // widget (contextmenu remonte à Canvas) — il ne doit ni sélectionner,
    // ni démarrer un drag, ni déposer un morph.
    if (e.button !== 0) return;
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
    // Snapshot des cibles de snap (figé pendant tout le drag — pas de get() par frame).
    snapTargets = buildSnapTargets(get(sceneStore), w.id);
    // Afficher les guides d'alignement (dashboard uniquement).
    afficherGuidesDrag({ x: w.x, y: w.y, width: w.largeur, height: w.hauteur });
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
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
  }

  async function onpointerup(e: PointerEvent) {
    if (!dragging) return;
    dragging = false;
    snapTargets = null;
    masquerGuides();
    (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId);
    await commitScene();
  }

  // ===== Boutons -/+ de plan (survol, dashboard uniquement) =====
  // « − » envoie tout en arrière-plan, « + » tout au premier plan
  // (mettreWidgetPlan, scene.ts). La caméra est exclue (plage réservée
  // z=5000 — isCamera masque le bloc). Disabled aux extrémités de la bande
  // non-caméra : un widget à égalité de z peut encore être (dé)coiffé.
  let estDerriere = $derived(
    !$sceneStore.widgets.some((x) => x.type !== "camera" && x.z < w.z)
  );
  let estDevant = $derived(
    !$sceneStore.widgets.some((x) => x.type !== "camera" && x.z > w.z)
  );

  // Double-clic sur le widget → ouvre la modale titres, focus sur le champ
  // du bord le plus proche du clic (haut/bas/gauche/droite).
  function ondblclick(e: MouseEvent) {
    e.stopPropagation();
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const relX = (e.clientX - rect.left) / rect.width;
    const relY = (e.clientY - rect.top) / rect.height;
    const dHaut = relY;
    const dBas = 1 - relY;
    const dGauche = relX;
    const dDroite = 1 - relX;
    const min = Math.min(dHaut, dBas, dGauche, dDroite);
    const position: PositionTitre =
      min === dHaut ? "haut" : min === dBas ? "bas" : min === dGauche ? "gauche" : "droite";
    titreModalOpen.set({ id: w.id, position });
  }
</script>

<!-- Outer = hit-box rectangle NON incliné (drag position + poignées resize). -->
<div
  class="widget"
  class:dragging
  class:morphable
  data-widget-id={w.id}
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
      <div class="chat-actif">{$t("widget.chatActif")}</div>
    {:else if isCamera}
      <!-- Widget caméra (dashboard) : placeholder seulement. L'image vient de
           la source OBS "SOS-Caméra" (dshow_input sous SOS-Diffusion) qui
           apparaît à travers le trou du widget dans diffusion.html. Le
           dashboard est une config, pas un player. -->
      <div class="camera-actif">
        <span>{$t("widget.camera")}</span>
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
        <span class="msg-pill-text">{$t("widget.inputViewerObs")}</span>
      </div>
    {:else if isSpeedrun}
      <!-- Widget speedrun splitter (dashboard) : timer + contrôles manuels +
           segments LSS + settings ASL. Le rendu diffusion est dans
           diffusion.html (OBS :4321) via WS {"type":"speedrun-etat"}. -->
      {@render mediaFond()}
      <div class="contenu-dessus">
        <WidgetSpeedrun contenu={w} gradient={cadreGradient} />
      </div>
    {:else if isGoal}
      <!-- Widget objectif (dashboard) : vignette live actuel/cible + barre.
           Le rendu complet + la célébration à l'atteinte sont dans
           diffusion.html (OBS :4321) via WS communaute/viewers/goal-atteint. -->
      {@render mediaFond()}
      <div class="goal-preview">
        <span class="goal-preview-label">
          {#if w.goalId}{goalLabelLie} · twitch{:else}{$t("goal.metrique." + (w.goalMetrique ?? "followers"))} · {w.goalPlateforme ?? "twitch"}{/if}
        </span>
        <span class="goal-preview-count">
          {goalActuel === undefined ? "N/A" : goalActuel.toLocaleString($localeTag)}
          {#if goalCibleLive}<span class="goal-preview-cible">/ {goalCibleLive.toLocaleString($localeTag)}</span>{/if}
        </span>
        {#if goalLieLive?.termine}
          <span class="goal-preview-label">{$t("carteEdition.goalTermine")}</span>
        {/if}
        {#if goalPct !== null}
          <div class="goal-preview-bar"><div class="goal-preview-fill" style="width:{goalPct}%"></div></div>
        {/if}
      </div>
    {:else if isMusique}
      <!-- Widget musique (dashboard) : miroir 1:1 du mini-lecteur Winamp
           de diffusion.html (LED temps + LCD marquee + EQ + posbar) —
           composant dédié, tick local 250 ms sur les stores musique. -->
      {@render mediaFond()}
      <WidgetMusique {w} />
    {:else if slidesList && slideCourante}
      <!-- Carrousel d'images : preview du slide courant (le fondu enchaîné
           n'est rendu que côté diffusion — ici swap de src simple). -->
      {#if aMorphs}
        <!-- Rendu morphé : canvas GL, source cachée = slide courante -->
        <canvas bind:this={morphCanvasEl} class="preview morph-canvas" draggable="false"></canvas>
        <img
          bind:this={imgEl}
          class="src-cachee"
          src={MEDIA_BASE + slideCourante.rel}
          alt=""
          draggable="false"
          crossorigin="anonymous"
          onload={() => (mediaPret += 1)}
        />
      {:else}
        <img
          class="preview"
          src={MEDIA_BASE + slideCourante.rel}
          alt=""
          draggable="false"
          style="object-fit:{fitCss}; object-position:{fit === 'centrer' ? 'center' : '50% 50%'}; transform:{mediaTransform}; transform-origin:center center; filter:{mediaFilter};"
        />
      {/if}
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
      <div class="lecture-obs msg-pill">
        <span class="msg-pill-icon">▶</span>
        <span class="msg-pill-text">{$t("widget.videoLectureObs")}</span>
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
          largeur={w.largeur}
          hauteur={w.hauteur}
          strokeWidth={cadreStroke}
          couleur={cadre.couleur}
          couleurFin={cadre.couleurFin}
          gradientAngle={cadre.gradientAngle}
          animer={cadre.animer === true}
          idCadre="w-{w.id}"
        />
      </div>
    {/if}
  </div>

  <!-- Titres du widget (haut/bas/gauche/droite — latéraux verticaux) — hors
       .widget-3d pour ne pas être clippés par le cadre SVG. Gradient de
       texte reprenant les couleurs du cadre actif (fallback blanc→gris).
       Police Google Font + taille px, style partagé entre les 4 positions. -->
  {#if titreHaut}
    <div class="widget-titre haut" style={titreStyle}>{titreHaut}</div>
  {/if}
  {#if titreBas}
    <div class="widget-titre bas" style={titreStyle}>{titreBas}</div>
  {/if}
  {#if titreGauche}
    <div class="widget-titre gauche" style={titreStyle}>{titreGauche}</div>
  {/if}
  {#if titreDroite}
    <div class="widget-titre droite" style={titreStyle}>{titreDroite}</div>
  {/if}

  <!-- Bandes de double-clic (haut/bas/gauche/droite) — affichées au survol de
       tout widget (avec ou sans titre existant) pour indiquer où
       double-cliquer pour ajouter/modifier un titre. -->
  <div class="titre-zone titre-zone-haut"><span>Titre (double-clic)</span></div>
  <div class="titre-zone titre-zone-bas"><span>Titre (double-clic)</span></div>
  <div class="titre-zone titre-zone-gauche"><span>Titre (double-clic)</span></div>
  <div class="titre-zone titre-zone-droite"><span>Titre (double-clic)</span></div>

  <!-- Boutons -/+ de plan : visibles au survol du widget (dashboard
       uniquement — jamais en diffusion). pointerdown/dblclick stoppés : un
       clic ne doit ni sélectionner, ni démarrer un drag, ni déposer un
       morph, ni ouvrir la modale titres. -->
  {#if !isCamera}
    <!-- Contre-scale : taille CONSTANTE à l'écran quel que soit le zoom du
         canvas (le badge est enfant du widget, il subirait le scale sinon).
         padding-bottom dans la boîte → l'écart visuel reste constant lui
         aussi. -->
    <div class="z-controls" style="transform: scale({scale > 0 ? 1 / scale : 1});">
      <button
        class="z-btn"
        title={$t("carteEdition.arrierePlan")}
        aria-label={$t("carteEdition.arrierePlan")}
        disabled={estDerriere}
        onpointerdown={(e) => e.stopPropagation()}
        ondblclick={(e) => e.stopPropagation()}
        onclick={() => void mettreWidgetPlan(w.id, "arriere")}
      >−</button>
      <button
        class="z-btn"
        title={$t("carteEdition.premierPlan")}
        aria-label={$t("carteEdition.premierPlan")}
        disabled={estDevant}
        onpointerdown={(e) => e.stopPropagation()}
        ondblclick={(e) => e.stopPropagation()}
        onclick={() => void mettreWidgetPlan(w.id, "avant")}
      >+</button>
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
  /* Badge « input viewer affiché dans OBS » (dashboard) : pastille centrée
     .msg-pill — teinte orange via la règle globale app.css (même couleur
     que .lecture-obs / .lecture-obs-fond, recette unifiée). */
  /* Vignette objectif (dashboard) : métrique·plateforme + actuel/cible +
     barre de progression — variables du thème uniquement. */
  .goal-preview {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 0.3rem;
    padding: 0.5rem;
    pointer-events: none;
    color: var(--texte);
  }
  .goal-preview-label {
    font-size: 0.72rem;
    color: var(--gris);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .goal-preview-count {
    font-size: 1.15rem;
    font-weight: 700;
  }
  .goal-preview-cible {
    color: var(--gris);
    font-weight: 500;
  }
  .goal-preview-bar {
    width: 80%;
    height: 5px;
    border-radius: 999px;
    background: var(--btn-inset);
    overflow: hidden;
  }
  .goal-preview-fill {
    height: 100%;
    background: var(--accent-ui);
    border-radius: 999px;
    transition: width 0.4s ease;
  }
  /* Vignette widget musique (dashboard) : le CSS mu-* vit dans
     WidgetMusique.svelte — miroir du bloc diffusion.html. */
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
     teinte orange partagée (.lecture-obs, règle globale). Dans .widget-3d
     → suit le tilt 3D et est découpé par le clip-path du cadre. */
  /* ===== Titres du widget ===== */
  /* Positionnés autour du widget : haut/bas centrés horizontalement,
     gauche/droite centrés verticalement en TEXTE VERTICAL (writing-mode).
     Hors .widget-3d → non clippés par le cadre SVG. pointer-events:none
     pour ne pas bloquer le drag/sélection. */
  .widget-titre {
    position: absolute;
    white-space: nowrap;
    pointer-events: none;
    z-index: 10;
    line-height: 1.1;
    font-weight: 400;
    text-align: center;
    /* Contour des lettres (couleur + épaisseur réglables) : inline dans le
       template. paint-order:stroke paint le contour SOUS le gradient de
       remplissage (sinon il mange les glyphes). */
    paint-order: stroke fill;
  }
  .widget-titre.haut,
  .widget-titre.bas {
    left: 50%;
    transform: translateX(-50%);
  }
  .widget-titre.haut {
    bottom: 100%;
    margin-bottom: 2px;
  }
  .widget-titre.bas {
    top: 100%;
    margin-top: 2px;
  }
  .widget-titre.gauche,
  .widget-titre.droite {
    top: 50%;
    transform: translateY(-50%);
    writing-mode: vertical-rl;
  }
  .widget-titre.gauche {
    right: 100%;
    margin-right: 2px;
  }
  .widget-titre.droite {
    left: 100%;
    margin-left: 2px;
  }
  /* Bandes de double-clic : fines bandes ambre (--message-user-action-color,
     sémantique « action » comme les z-controls) le long des 4 bords intérieurs
     — haut/bas pleine largeur, gauche/droite entre les deux — affichées au
     :hover de tout widget, avec ou sans titre existant. Le seul libellé
     est le mot « Titre » (vertical sur les côtés). pointer-events:none (le
     dblclick est géré par le parent). */
  .titre-zone {
    position: absolute;
    display: flex;
    align-items: center;
    justify-content: center;
    pointer-events: none;
    z-index: 3;
    opacity: 0;
    transition: opacity 0.15s ease;
    background: color-mix(in srgb, var(--message-user-action-color) 10%, transparent);
    border: 1px dashed color-mix(in srgb, var(--message-user-action-color) 45%, transparent);
    color: var(--message-user-action-color);
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.15em;
    text-transform: uppercase;
    overflow: hidden;
  }
  .titre-zone-haut {
    top: 0;
    left: 0;
    width: 100%;
    height: 7%;
  }
  .titre-zone-bas {
    bottom: 0;
    left: 0;
    width: 100%;
    height: 7%;
  }
  .titre-zone-gauche {
    left: 0;
    top: 7%;
    height: 86%;
    width: 7%;
  }
  .titre-zone-droite {
    right: 0;
    top: 7%;
    height: 86%;
    width: 7%;
  }
  .titre-zone-gauche span,
  .titre-zone-droite span {
    writing-mode: vertical-rl;
  }
  .widget:hover .titre-zone { opacity: 1; }

  /* ===== Boutons -/+ de plan ===== */
  /* Badge flottant AU-DESSUS du coin supérieur droit du widget (ne recouvre
     pas le contenu), visible au survol — dashboard uniquement. Le bloc est
     contre-scalé inline (transform 1/scale) : tout ce qui suit est en px
     ÉCRAN constants, pas en px canvas. Ambre --message-user-action-color
     (sémantique « action », famille des titre-zones) — voyant sans casser
     le thème. pointer-events:none au repos → la zone invisible ne mange
     pas les clics ; masqué pendant le drag/resize. */
  .z-controls {
    position: absolute;
    bottom: 100%;
    right: 0;
    transform-origin: bottom right;
    display: flex;
    gap: 6px;
    padding: 0 0 6px 0;
    opacity: 0;
    pointer-events: none;
    transition: opacity 0.15s ease;
    z-index: 30;
  }
  .widget:hover .z-controls {
    opacity: 1;
    pointer-events: auto;
  }
  .widget.dragging .z-controls {
    opacity: 0;
    pointer-events: none;
  }
  .z-btn {
    width: 30px;
    height: 30px;
    padding: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 19px;
    font-weight: 700;
    line-height: 1;
    color: var(--message-user-action-color);
    background: color-mix(in srgb, var(--message-user-action-color) 22%, var(--fond-panneau));
    border: 2px solid var(--message-user-action-color);
    border-radius: 50%;
    box-shadow: 0 0 8px color-mix(in srgb, var(--message-user-action-color) 45%, transparent);
    cursor: pointer;
    transition:
      background 0.12s ease,
      color 0.12s ease,
      box-shadow 0.12s ease;
  }
  .z-btn:hover:not(:disabled) {
    color: var(--fond);
    background: var(--message-user-action-color);
    box-shadow: 0 0 14px color-mix(in srgb, var(--message-user-action-color) 70%, transparent);
  }
  .z-btn:disabled {
    opacity: 0.35;
    cursor: default;
  }
</style>
