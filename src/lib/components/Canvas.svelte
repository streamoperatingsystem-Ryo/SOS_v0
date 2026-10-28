<script lang="ts">
  import { onMount } from "svelte";
  import {
    sceneStore, selectWidget, selectedIdStore, ajouterMorphFond, importFond, clearFond,
    setBgFit, toggleBgMiroir, resetFondTransformation, resetFondEffets,
    resetMorphsFond, setBgPaused, retirerFondCapture,
    dupliquerWidget, setMediaFit, toggleMediaMiroir, rotateWidgetLocal,
    commitScene, setWidgetMediaPaused, resetMediaTransformation,
    resetMediaEffets, resetMorphsWidget, importMedia, clearWidgetMedia,
    setWidgetTrou, deleteObsTrouSource, transformerEnFlottant,
    transformerEnWidget, mettreWidgetPlan, widgetPleinEcran, setCameraCachee,
  } from "../stores/scene";
  import {
    setCarteEdition, titreFondModalOpen, titreModalOpen, confirmDeleteWidget,
    cadreModalOpen,
  } from "../stores/ui";
  import { registerVideo, unregisterVideo } from "../stores/video";
  import { connexions, type Connexions } from "../stores/chat";
  import { morphMode, morphIntensite, morphRayon, morphSens } from "../stores/morph";
  import { creerRenduMorph, dessinerFitCanvas, chaineFilter, type RenduMorph } from "../morph/gl-morph";
  import WidgetComp from "./Widget.svelte";
  import SelectionOverlay from "./SelectionOverlay.svelte";
  import ModuleHud from "./ModuleHud.svelte";
  import { MODULE_IDS } from "../tauri";
  import AlignmentGuides from "./AlignmentGuides.svelte";
  import CadreSVG from "./CadreSVG.svelte";
  import ObsSourceDialog from "./ObsSourceDialog.svelte";
  import CtxMenu from "./CtxMenu.svelte";
  import { gradientCadre } from "../cadres/registre";
  import { POLICE_TITRE_DEFAUT, TAILLE_TITRE_DEFAUT, ESPACEMENT_TITRE_DEFAUT } from "../fonts";
  import { t } from "../stores/langue";

  // Dims canvas = résolution OBS lue au connect (fallback 1920×1080).
  // Un vrai changement de résolution (> 10 %) rescale la scène côté Rust
  // (scenes::adapter_canvas) ; en dessous du seuil, la géométrie ne bouge pas.
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

  // Widget sélectionné (poignées resize rendues dans SelectionOverlay,
  // frère des .widget — jamais couvertes par un voisin à z supérieur).
  let widgetSelectionne = $derived(
    $sceneStore.widgets.find((x) => x.id === $selectedIdStore)
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
  // Effets visuels du fond (luminosité/contraste/teinte/flou). La pixelisation
  // est rendue en canvas : chemin morph via rendreBgMorph, chemin DOM via un
  // canvas overlay peint par rendreBgPixel (même formule que diffusion.html).
  let bgLum = $derived($sceneStore.bgLum ?? 0);
  let bgContraste = $derived($sceneStore.bgContraste ?? 0);
  let bgTeinte = $derived($sceneStore.bgTeinte ?? 0);
  let bgFlou = $derived($sceneStore.bgFlou ?? 0);
  let bgPixel = $derived($sceneStore.bgPixel ?? 0);
  // Miroir du fond : flip H/V du média — scale ±1 dans l'espace local du
  // média (après rotate dans la liste = flip de la source). Le zoom n'est
  // plus ici : il est sur .bg-layer (post-tilt, voir bgTiltStyle).
  let bgMiroirH = $derived($sceneStore.bgMiroirH === true);
  let bgMiroirV = $derived($sceneStore.bgMiroirV === true);
  // Zoom + tilt 3D du fond (gizmo, ±90°) : transform sur .bg-layer, identique
  // à celui posé sur #bg en diffusion. scale(z) EN PREMIER dans la liste =
  // appliqué EN DERNIER (espace écran, post-perspective) → le zoom agrandit
  // le plan tilté entier et couvre tout le canvas même incliné.
  // d = canvasW×2 : même ratio d/demi-largeur que les widgets (800px pour
  // ~400px) → tilt doux ; le bord ne croise jamais le plan de fuite.
  let bgTiltStyle = $derived.by(() => {
    const rx = $sceneStore.bgRotX ?? 0;
    const ry = $sceneStore.bgRotY ?? 0;
    const z = bgZoom;
    return rx !== 0 || ry !== 0 || z !== 1
      ? `transform:scale(${z}) perspective(${canvasW * 2}px) rotateX(${rx}deg) rotateY(${ry}deg); transform-origin:center center;`
      : "";
  });
  let bgFilter = $derived(chaineFilter(bgLum, bgContraste, bgTeinte, bgFlou));
  // Fond = capture OBS (app/jeu plein canvas) : prioritaire sur le média.
  let bgObsSource = $derived($sceneStore.bgObsSource ?? null);
  // ===== Carrousel d'images du fond (bgSlides) =====
  // Liste non vide = carrousel actif (prioritaire sur bgMedia qui reste
  // stocké ; bgObsSource garde la priorité absolue — même règle que la
  // diffusion). Preview = slide courante avancée par un intervalle LOCAL ;
  // le fondu enchaîné n'est rendu que côté :4321 — ici swap de src simple.
  let bgSlidesList = $derived(
    $sceneStore.bgSlides && $sceneStore.bgSlides.length > 0 && !bgObsSource
      ? $sceneStore.bgSlides
      : null
  );
  let bgSlideIdx = $state(0);
  let bgSlideCourante = $derived(
    bgSlidesList ? bgSlidesList[Math.min(bgSlideIdx, bgSlidesList.length - 1)] : null
  );
  $effect(() => {
    // Liste changée (ajout/retrait/réordonnancement) → repartir au début.
    void bgSlidesList;
    bgSlideIdx = 0;
  });
  $effect(() => {
    if (!bgSlidesList || bgSlidesList.length < 2) return;
    const ms = Math.max(1, $sceneStore.bgSlidesSec ?? 5) * 1000;
    const timer = setInterval(() => {
      bgSlideIdx = (bgSlideIdx + 1) % bgSlidesList.length;
    }, ms);
    return () => clearInterval(timer);
  });
  let hasFond = $derived(bgMedia.length > 0 || !!bgObsSource || !!bgSlidesList);
  // ===== Spots lumineux (modules.spots) — aperçu statique, pas de carte =====
  // Effet plein canvas rendu en diffusion ; ici deux cônes figés sur le
  // fond (même tilt que .bg-layer) pour marquer que le module est actif.
  // Pas de carte ModuleHud : un rectangle plein canvas bloquerait le drag.
  let spotsModule = $derived($sceneStore.modules?.spots);
  let spotsActif = $derived(spotsModule?.actif === true);
  let spotsOpacite = $derived(
    typeof spotsModule?.opacite === "number"
      ? Math.min(1, Math.max(0, spotsModule.opacite))
      : 0.8
  );
  // ===== Aides contextuelles (presets guidés — dashboard uniquement) =====
  // Encadrés jaune-orangé auto-masqués : aideFond disparaît dès qu'un média
  // ou une capture OBS est lié au fond ; aideObjectifs disparaît quand
  // TOUTES les plateformes des widgets goal sont connectées (store
  // connexions — même clés que goalPlateforme) ; aideCadres disparaît à la
  // première ouverture de la modale Cadres. Les encadrés centrés (fond +
  // cadres) sont empilés dans .aides-centre. Jamais rendus en diffusion.
  let aideFondVisible = $derived($sceneStore.aideFond === true && !hasFond);
  // Boîte englobante des widgets goal : l'encadré se pose juste au-dessus.
  let goalsBox = $derived.by(() => {
    const gs = $sceneStore.widgets.filter((w) => w.type === "goal");
    if (gs.length === 0) return null;
    let x0 = Infinity, y0 = Infinity, x1 = -Infinity;
    for (const g of gs) {
      x0 = Math.min(x0, g.x);
      y0 = Math.min(y0, g.y);
      x1 = Math.max(x1, g.x + g.largeur);
    }
    return { cx: (x0 + x1) / 2, top: y0 };
  });
  let goalsPlateformes = $derived(
    new Set(
      $sceneStore.widgets
        .filter((w) => w.type === "goal" && w.goalPlateforme)
        .map((w) => w.goalPlateforme as string)
    )
  );
  let aideObjectifsVisible = $derived(
    $sceneStore.aideObjectifs === true &&
      !!goalsBox &&
      goalsPlateformes.size > 0 &&
      ![...goalsPlateformes].every((p) => $connexions[p as keyof Connexions])
  );
  let aideCadresVisible = $derived($sceneStore.aideCadres === true);
  // Auto-masquage « mission accomplie » : la première ouverture de la modale
  // Cadres agit comme la croix — dismiss persistant (commit scène).
  $effect(() => {
    if ($cadreModalOpen && aideCadresVisible) fermerAide("aideCadres");
  });
  // Croix de fermeture : dismiss PERSISTANT — le flag repasse à false et est
  // commité (l'aide ne revient pas au reload). La croix est le seul élément
  // cliquable de l'encadré (parent pointer-events:none).
  function fermerAide(champ: "aideFond" | "aideObjectifs" | "aideCadres") {
    sceneStore.update((s) => ({ ...s, [champ]: false }));
    void commitScene();
  }
  let bgIsVideo = $derived(bgKind === "video");
  // Badge « en lecture dans OBS » (fond) : bgPaused === false = la vidéo de
  // fond joue réellement côté diffusion (:4321) — vignette dashboard figée.
  let bgLecture = $derived(bgIsVideo && ($sceneStore.bgPaused ?? true) === false);
  let bgPaused = $derived($sceneStore.bgPaused ?? true);

  // Modes d'affichage proposés (même liste que la carte édition).
  const FIT_MODES = ["ajuster", "remplir", "etendre", "etirer", "centrer", "vignette"];
  // Extensions vidéo (même liste que Widget.svelte — kindFromMedia local).
  const VIDEO_EXT = ["mp4", "webm"];
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
  let bgMediaTransform = $derived(`translate(${bgOx}px, ${bgOy}px) rotate(${bgRot}deg) scale(${bgMiroirH ? -1 : 1}, ${bgMiroirV ? -1 : 1})`);

  // ===== Cadre unifié (widgets + bord extérieur du canvas) =====
  // Source unique : cadreWidget (champ canonique) — le même cadre que les
  // widgets. Le bord du canvas n'est plus qu'une cible de rendu, masquable
  // par bordActif (undefined = true, rétro-compat vieilles scènes).
  let cadre = $derived($sceneStore.cadreWidget ?? null);
  let cadreActif = $derived(!!cadre?.actif && (cadre?.strokeWidth ?? 0) > 0);
  let cadreBordActif = $derived(cadreActif && (cadre?.bordActif ?? true));
  // Le badge « en lecture dans OBS » (fond) est orange fixe
  // (.lecture-obs-fond — recette .msg-pill, app.css) — jamais la couleur
  // du cadre.

  // ===== Titre du fond de l'application (intérieur haut/bas) =====
  // Titres du fond (haut/bas/gauche/droite — intérieur du fond, latéraux en
  // texte vertical). Jusqu'à 4 simultanés, style partagé (police/taille/…).
  // Le gradient du texte reprend les couleurs du cadre unifié
  // actif (fallback blanc→gris si aucun cadre actif).
  let bgTitreHaut = $derived($sceneStore.bgTitreHaut ?? "");
  let bgTitreBas = $derived($sceneStore.bgTitreBas ?? "");
  let bgTitreGauche = $derived($sceneStore.bgTitreGauche ?? "");
  let bgTitreDroite = $derived($sceneStore.bgTitreDroite ?? "");
  let hasBgTitre = $derived(!!(bgTitreHaut || bgTitreBas || bgTitreGauche || bgTitreDroite));
  let bgTitrePolice = $derived($sceneStore.bgTitrePolice ?? POLICE_TITRE_DEFAUT);
  let bgTitreTaille = $derived($sceneStore.bgTitreTaille ?? TAILLE_TITRE_DEFAUT);
  let bgTitreGras = $derived($sceneStore.bgTitreGras ?? false);
  let bgTitreItalique = $derived($sceneStore.bgTitreItalique ?? false);
  let bgTitreSouligne = $derived($sceneStore.bgTitreSouligne ?? false);
  let bgTitreEspacement = $derived($sceneStore.bgTitreEspacement ?? ESPACEMENT_TITRE_DEFAUT);
  let bgTitreContourCouleur = $derived($sceneStore.bgTitreContourCouleur ?? "#fff");
  let bgTitreContourEpaisseur = $derived($sceneStore.bgTitreContourEpaisseur ?? 2);
  // Dégradé du texte : personnalisé si bgTitreCouleur défini, sinon cadre.
  let bgTitreGradient = $derived(
    $sceneStore.bgTitreCouleur
      ? { from: $sceneStore.bgTitreCouleur, to: $sceneStore.bgTitreCouleurFin ?? $sceneStore.bgTitreCouleur }
      : gradientCadre(cadreActif ? cadre : null)
  );
  let bgTitreFontStyle = $derived(bgTitreItalique ? "italic" : "normal");
  let bgTitreFontWeight = $derived(bgTitreGras ? "700" : "400");
  let bgTitreTextDecoration = $derived(bgTitreSouligne ? "underline" : "none");
  // Style inline partagé par les 4 titres du fond.
  let bgTitreStyle = $derived(
    "font-family: '" + bgTitrePolice + "', sans-serif; font-size: " + bgTitreTaille +
    "px; font-style: " + bgTitreFontStyle + "; font-weight: " + bgTitreFontWeight +
    "; text-decoration: " + bgTitreTextDecoration + "; letter-spacing: " + bgTitreEspacement +
    "px; -webkit-text-stroke: " + bgTitreContourEpaisseur + "px " + bgTitreContourCouleur +
    "; background: linear-gradient(135deg, " + bgTitreGradient.from + ", " + bgTitreGradient.to +
    "); -webkit-background-clip: text; background-clip: text; color: transparent;"
  );

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
    // Bouton gauche uniquement : le clic droit ouvre le menu contextuel du
    // fond (oncontextmenu) — il ne doit ni désélectionner ni déposer un morph.
    if (e.button !== 0) return;
    if ($morphMode && hasFond && (!bgIsVideo || bgSlidesList)) {
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
    if ($morphMode && hasFond && (!bgIsVideo || bgSlidesList)) {
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

  // ===== Menus contextuels (clic droit sur le canvas) =====
  // Wrapper CtxMenu (recette .ctx-menu de app.css) : fixed à la position
  // souris, fermeture clic extérieur + Escape et clamping viewport gérés
  // par le composant. Deux menus mutuellement exclusifs pilotés par
  // ctxWidgetId : null = menu FOND, sinon menu du widget identifié
  // (data-widget-id posé sur .widget — contextmenu bouillonne jusqu'à
  // .canvas). confirmDeleteFond = confirmation inline Oui/Non avant
  // clearFond.
  let fondMenuOpen = $state(false);
  let fondMenuX = $state(0);
  let fondMenuY = $state(0);
  let confirmDeleteFond = $state(false);
  let ctxWidgetId = $state<string | null>(null);
  // Modale capture app/jeu en fond (ObsSourceDialog mode "fond", montée ici
  // comme dans CarteEdition — la carte possède son propre état local).
  let obsFondDialogOpen = $state(false);
  // Modale source OBS d'un widget trou (ObsSourceDialog mode "widget").
  let obsWidgetDialogId = $state<string | null>(null);

  function onCanvasContextMenu(e: MouseEvent) {
    e.preventDefault();
    // Clic droit sur un widget (ou ses annexes : badge de survol .wbadge,
    // poignées .sel-overlay — toutes portent data-widget-id) : menu
    // WIDGET — le widget est sélectionné pour que les actions qui visent
    // selectedId (importMedia, carte, touche Suppr) tombent juste.
    const wEl = (e.target as HTMLElement).closest("[data-widget-id]");
    const wId = wEl instanceof HTMLElement ? (wEl.dataset.widgetId ?? null) : null;
    if (wEl && !wId) return;
    if (wId) selectWidget(wId);
    ctxWidgetId = wId;
    fondMenuX = e.clientX;
    fondMenuY = e.clientY;
    confirmDeleteFond = false;
    fondMenuOpen = true;
  }

  function fermerFondMenu() {
    fondMenuOpen = false;
    confirmDeleteFond = false;
    ctxWidgetId = null;
  }

  // Toggles : gardent le menu ouvert (essai rapide des réglages).
  // Actions : ferment le menu (pattern SceneBar).
  function ctxFit(mode: string) { void setBgFit(mode); }
  function ctxMiroir(sens: "h" | "v") { void toggleBgMiroir(sens); }
  function ctxToggleLecture() { void setBgPaused(!bgPaused); }
  function ctxToggleMorph() { morphMode.update((m) => !m); }
  async function ctxImportFond() { fermerFondMenu(); await importFond(); }
  function ctxObsFond() { fermerFondMenu(); obsFondDialogOpen = true; }
  async function ctxRetirerCapture() { fermerFondMenu(); await retirerFondCapture(); }
  function ctxTitre() { fermerFondMenu(); titreFondModalOpen.set(true); }
  async function ctxResetTransfo() { fermerFondMenu(); await resetFondTransformation(); }
  async function ctxResetEffets() { fermerFondMenu(); await resetFondEffets(); }
  async function ctxResetMorphs() { fermerFondMenu(); await resetMorphsFond(); }
  async function ctxSupprimerConfirm() { fermerFondMenu(); await clearFond(); }

  // ===== Menu contextuel WIDGET (clic droit sur un widget) =====
  // Contenu miroir de la carte widget (CarteEdition, cible "widget") — mêmes
  // conditions : branche média (trou/flottant) = types hors chat/input-viewer/
  // speedrun/goal/camera ; morphable = type media + média image (le toggle
  // active le mode morphing global, le clic suivant dépose le morph).
  let ctxWidget = $derived(ctxWidgetId ? $sceneStore.widgets.find((x) => x.id === ctxWidgetId) : undefined);
  let wgType = $derived(ctxWidget?.type ?? "media");
  let wgMedia = $derived(ctxWidget?.media ?? "");
  let wgHasMedia = $derived(wgMedia.length > 0);
  let wgKind = $derived(
    ctxWidget?.kind ??
      (wgMedia && VIDEO_EXT.includes(wgMedia.split(".").pop()?.toLowerCase() ?? "") ? "video" : "image")
  );
  let wgIsVideo = $derived(wgKind === "video");
  let wgIsTrou = $derived(ctxWidget?.trou === true);
  let wgHasObsSource = $derived(!!ctxWidget?.obsSource);
  let wgFlottant = $derived(ctxWidget?.sansCadre === true);
  let wgFit = $derived(ctxWidget?.mediaFit ?? "ajuster");
  let wgMiroirH = $derived(ctxWidget?.mediaMiroirH === true);
  let wgMiroirV = $derived(ctxWidget?.mediaMiroirV === true);
  let wgPaused = $derived(ctxWidget?.mediaPaused ?? true);
  let wgHasMorphs = $derived((ctxWidget?.morphs?.length ?? 0) > 0);
  let wgIsBrancheMedia = $derived(!["chat", "input-viewer", "speedrun", "goal", "camera"].includes(wgType));
  let wgMorphable = $derived(wgType === "media" && wgHasMedia && !wgIsVideo);

  // Toggles : gardent le menu ouvert. Actions : ferment le menu.
  function ctxWgFit(id: string, mode: string) { void setMediaFit(id, mode); }
  function ctxWgMiroirMedia(id: string, sens: "h" | "v") { void toggleMediaMiroir(id, sens); }
  function ctxWgRetourne(id: string, sens: "h" | "v" | "reset") {
    rotateWidgetLocal(id, sens === "v" ? 180 : 0, sens === "h" ? 180 : 0);
    void commitScene();
  }
  function ctxWgLecture(id: string) { void setWidgetMediaPaused(id, !wgPaused); }
  function ctxWgTrou(id: string) { void setWidgetTrou(id, !wgIsTrou); }
  function ctxWgMorph() { morphMode.update((m) => !m); }
  function ctxWgTitre(id: string) { fermerFondMenu(); titreModalOpen.set({ id }); }
  async function ctxWgDupliquer(id: string) { fermerFondMenu(); await dupliquerWidget(id); }
  async function ctxWgDevant(id: string) { fermerFondMenu(); await mettreWidgetPlan(id, "avant"); }
  async function ctxWgDerriere(id: string) { fermerFondMenu(); await mettreWidgetPlan(id, "arriere"); }
  // Œil caméra : l'état est lu AVANT fermerFondMenu (ctxWidgetId → null
  // vide ctxWidget). Action → ferme le menu (pattern établi).
  async function ctxWgCameraCachee(id: string) {
    const cachee = ctxWidget?.cameraCachee === true;
    fermerFondMenu();
    await setCameraCachee(id, !cachee);
  }
  async function ctxWgPleinEcran(id: string) { fermerFondMenu(); await widgetPleinEcran(id); }
  async function ctxWgResetTransfo(id: string) { fermerFondMenu(); await resetMediaTransformation(id); }
  async function ctxWgResetEffets(id: string) { fermerFondMenu(); await resetMediaEffets(id); }
  async function ctxWgResetMorphs(id: string) { fermerFondMenu(); await resetMorphsWidget(id); }
  // importMedia vise selectedId — le widget est sélectionné à l'ouverture du menu.
  async function ctxWgImportMedia() { fermerFondMenu(); await importMedia(); }
  async function ctxWgSupprMedia(id: string) { fermerFondMenu(); await clearWidgetMedia(id); }
  function ctxWgObsDialog(id: string) { fermerFondMenu(); obsWidgetDialogId = id; }
  async function ctxWgSupprObsSource(id: string) { fermerFondMenu(); await deleteObsTrouSource(id); }
  async function ctxWgFlottant(id: string) { fermerFondMenu(); await transformerEnFlottant(id); }
  async function ctxWgTransformerWidget(media: string) { fermerFondMenu(); await transformerEnWidget(media); }
  // Supprimer = la modale ConfirmDeleteWidgetModal existante (vraie
  // confirmation, pas le Oui/Non inline — pattern carte/App.touche Suppr).
  function ctxWgSupprimer() { fermerFondMenu(); confirmDeleteWidget.set(true); }

  // ===== Morphing du fond (rendu canvas WebGL si bgMorphs) =====
  // Même pattern que les widgets : média → offscreen (fit + transforms fond) →
  // warp GL → canvas visible. Fond dashboard = vignette figée → one-shot.
  let bgAMorphs = $derived(($sceneStore.bgMorphs?.length ?? 0) > 0 && hasFond);
  let bgMorphCanvasEl: HTMLCanvasElement | undefined = $state(undefined);
  let bgImgEl: HTMLImageElement | undefined = $state(undefined);
  let bgRendu: RenduMorph | undefined = $state(undefined);
  let bgOff: HTMLCanvasElement | undefined = $state(undefined);
  let bgPret = $state(0);
  let bgPixelCanvasEl: HTMLCanvasElement | undefined = $state(undefined);

  function rendreBgMorph(): void {
    if (!bgRendu) return;
    // Carrousel : la source est la slide courante (bgImgEl) — jamais la vidéo.
    const useVideo = bgIsVideo && !bgSlidesList;
    const src: HTMLImageElement | HTMLVideoElement | null = useVideo
      ? bgVideoEl ?? null
      : bgImgEl ?? null;
    if (!src) return;
    const sw = useVideo ? bgVideoEl!.videoWidth : bgImgEl!.naturalWidth;
    const sh = useVideo ? bgVideoEl!.videoHeight : bgImgEl!.naturalHeight;
    if (!sw || !sh) return;
    if (!bgOff) bgOff = document.createElement("canvas");
    if (bgOff.width !== canvasW) bgOff.width = canvasW;
    if (bgOff.height !== canvasH) bgOff.height = canvasH;
    const ctx = bgOff.getContext("2d");
    if (!ctx) return;
    ctx.clearRect(0, 0, canvasW, canvasH);
    ctx.save();
    // Effets visuels (filter + pixelisation) appliqués AVANT le warp GL —
    // même ordre que dessinerContenuFond (diffusion.html) / Widget.dessinerOffscreen.
    const bhs = bgMiroirH ? -1 : 1;
    const bvs = bgMiroirV ? -1 : 1;
    if (bgPixel > 0) {
      const pf = Math.max(1, bgPixel);
      const pw = Math.max(1, Math.round(canvasW / pf));
      const ph = Math.max(1, Math.round(canvasH / pf));
      const pix = document.createElement("canvas");
      pix.width = pw;
      pix.height = ph;
      const pctx = pix.getContext("2d");
      if (pctx) {
        pctx.filter = bgFilter;
        pctx.save();
        pctx.translate(pw / 2, ph / 2);
        pctx.translate(bgOx / pf, bgOy / pf);
        pctx.rotate((bgRot * Math.PI) / 180);
        pctx.scale(bhs, bvs);
        pctx.translate(-pw / 2, -ph / 2);
        dessinerFitCanvas(pctx, src, sw, sh, pw, ph, bgFitCss);
        pctx.restore();
        ctx.filter = "none";
        ctx.imageSmoothingEnabled = false;
        ctx.drawImage(pix, 0, 0, pw, ph, 0, 0, canvasW, canvasH);
        ctx.imageSmoothingEnabled = true;
        ctx.restore();
        bgRendu.rendre(bgOff, $sceneStore.bgMorphs ?? []);
        return;
      }
    }
    ctx.filter = bgFilter;
    ctx.translate(canvasW / 2, canvasH / 2);
    ctx.translate(bgOx, bgOy);
    ctx.rotate((bgRot * Math.PI) / 180);
    ctx.scale(bhs, bvs);
    ctx.translate(-canvasW / 2, -canvasH / 2);
    dessinerFitCanvas(ctx, src, sw, sh, canvasW, canvasH, bgFitCss);
    ctx.filter = "none";
    ctx.restore();
    bgRendu.rendre(bgOff, $sceneStore.bgMorphs ?? []);
  }

  // Rendu one-shot réactif : morphs, média, dims, transforms fond, prêt.
  $effect(() => {
    if (!bgAMorphs || !bgRendu) return;
    void bgPret;
    void $sceneStore.bgMorphs;
    void bgMedia;
    void bgSlideCourante; // repeindre à chaque slide du carrousel
    canvasW;
    canvasH;
    void bgRot;
    void bgOx;
    void bgOy;
    void bgFitCss;
    void bgFilter;
    void bgPixel;
    void bgMiroirH;
    void bgMiroirV;
    rendreBgMorph();
  });

  // ===== Pixelisation du fond SANS morph (chemin DOM) =====
  // Le img/video reste chargé comme source drawImage mais est masqué
  // (visibility:hidden) ; un canvas overlay est peint avec la même formule
  // que rendreBgMorph / dessinerContenuFond (diffusion.html). Aucun filter
  // ni transform CSS sur le canvas — tout est dessiné ici.
  function rendreBgPixel(): void {
    const cv = bgPixelCanvasEl;
    if (!cv) return;
    // Carrousel : la source est la slide courante (bgImgEl) — jamais la vidéo.
    const useVideo = bgIsVideo && !bgSlidesList;
    const src: HTMLImageElement | HTMLVideoElement | null = useVideo
      ? bgVideoEl ?? null
      : bgImgEl ?? null;
    if (!src) return;
    const sw = useVideo ? bgVideoEl!.videoWidth : bgImgEl!.naturalWidth;
    const sh = useVideo ? bgVideoEl!.videoHeight : bgImgEl!.naturalHeight;
    if (!sw || !sh) return;
    if (useVideo && bgVideoEl!.readyState < 2) return;
    if (cv.width !== canvasW) cv.width = canvasW;
    if (cv.height !== canvasH) cv.height = canvasH;
    const ctx = cv.getContext("2d");
    if (!ctx) return;
    ctx.clearRect(0, 0, canvasW, canvasH);
    ctx.save();
    const pf = Math.max(1, bgPixel);
    const pw = Math.max(1, Math.round(canvasW / pf));
    const ph = Math.max(1, Math.round(canvasH / pf));
    const pix = document.createElement("canvas");
    pix.width = pw;
    pix.height = ph;
    const pctx = pix.getContext("2d");
    if (!pctx) {
      ctx.restore();
      return;
    }
    pctx.filter = bgFilter;
    pctx.save();
    pctx.translate(pw / 2, ph / 2);
    pctx.translate(bgOx / pf, bgOy / pf);
    pctx.rotate((bgRot * Math.PI) / 180);
    pctx.scale(bgMiroirH ? -1 : 1, bgMiroirV ? -1 : 1);
    pctx.translate(-pw / 2, -ph / 2);
    dessinerFitCanvas(pctx, src, sw, sh, pw, ph, bgFitCss);
    pctx.restore();
    ctx.filter = "none";
    ctx.imageSmoothingEnabled = false;
    ctx.drawImage(pix, 0, 0, pw, ph, 0, 0, canvasW, canvasH);
    ctx.imageSmoothingEnabled = true;
    ctx.restore();
  }

  // Rendu one-shot réactif : pixel, média, dims, transforms fond, prêt.
  $effect(() => {
    if (bgAMorphs || bgPixel <= 0 || !bgPixelCanvasEl) return;
    void bgPret;
    void bgMedia;
    void bgSlideCourante; // repeindre à chaque slide du carrousel
    canvasW;
    canvasH;
    void bgRot;
    void bgOx;
    void bgOy;
    void bgFitCss;
    void bgFilter;
    void bgPixel;
    void bgMiroirH;
    void bgMiroirV;
    rendreBgPixel();
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
      class:morphable={$morphMode && hasFond && (!bgIsVideo || bgSlidesList) && !bgObsSource}
      style="width:{canvasW}px; height:{canvasH}px; transform: scale({scale});"
      onpointerdown={onCanvasPointerDown}
      onpointermove={onCanvasPointerMove}
      onpointerleave={onCanvasPointerLeave}
      oncontextmenu={onCanvasContextMenu}
      role="presentation"
    >
      {#if hasFond}
        <div class="bg-layer" style={bgTiltStyle}>
          {#if bgObsSource}
            <!-- Fond = capture OBS (app/jeu plein canvas) : rien n'est rendu
                 côté dashboard (la diffusion :4321 ne peint pas le fond non
                 plus — page transparente → la source SOS-Fond-* en bas de
                 pile OBS apparaît). Pastille informative (recette .msg-pill,
                 pattern .lecture-obs-fond). -->
            <div class="lecture-obs-fond msg-pill">
              <span class="msg-pill-icon">▶</span>
              <span class="msg-pill-text">{$t("canvas.captureFond")}</span>
            </div>
          {:else if bgAMorphs}
            <!-- Rendu morphé : canvas GL (le warp inclut fit + transforms fond) -->
            <canvas bind:this={bgMorphCanvasEl} class="bg-media morph-canvas" draggable="false"></canvas>
            {#if bgIsVideo && !bgSlidesList}
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
                src={MEDIA_BASE + (bgSlideCourante?.rel ?? bgMedia)}
                alt=""
                draggable="false"
                crossorigin="anonymous"
                onload={() => (bgPret += 1)}
              />
            {/if}
          {:else}
            {#if bgSlidesList && bgSlideCourante}
              <!-- Carrousel de fond : preview du slide courant (le fondu
                   enchaîné n'est rendu que côté diffusion — ici swap de src).
                   bgImgEl sert de source au chemin pixelisé, comme bgMedia. -->
              <img
                class="bg-media"
                class:bg-cachee={bgPixel > 0}
                bind:this={bgImgEl}
                src={MEDIA_BASE + bgSlideCourante.rel}
                alt=""
                draggable="false"
                onload={() => (bgPret += 1)}
                style="object-fit:{bgFitCss}; object-position:{bgFit === 'centrer' ? 'center' : '50% 50%'}; transform:{bgMediaTransform}; transform-origin:center center; filter:{bgFilter};"
              />
            {:else if bgIsVideo}
              <video
                class="bg-media bg-video"
                class:bg-cachee={bgPixel > 0}
                bind:this={bgVideoEl}
                muted
                playsinline
                preload="metadata"
                draggable="false"
                onloadeddata={() => (bgPret += 1)}
                onseeked={() => (bgPret += 1)}
                style="object-fit:{bgFitCss}; object-position:{bgFit === 'centrer' ? 'center' : '50% 50%'}; transform:{bgMediaTransform}; transform-origin:center center; filter:{bgFilter};"
              ></video>
            {:else}
              <img
                class="bg-media"
                class:bg-cachee={bgPixel > 0}
                bind:this={bgImgEl}
                src={MEDIA_BASE + bgMedia}
                alt=""
                draggable="false"
                onload={() => (bgPret += 1)}
                style="object-fit:{bgFitCss}; object-position:{bgFit === 'centrer' ? 'center' : '50% 50%'}; transform:{bgMediaTransform}; transform-origin:center center; filter:{bgFilter};"
              />
            {/if}
            {#if bgPixel > 0}
              <!-- Fond pixelisé : le média masqué sert de source drawImage ;
                   aucun filter/transform CSS sur le canvas (tout est peint). -->
              <canvas bind:this={bgPixelCanvasEl} class="bg-media bg-pixel-canvas" draggable="false"></canvas>
            {/if}
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
            <div class="lecture-obs-fond msg-pill">
              <span class="msg-pill-icon">▶</span>
              <span class="msg-pill-text">{$t("canvas.videoFondObs")}</span>
            </div>
          {/if}
        </div>
      {/if}

      <!-- Module « Spots lumineux » : aperçu STATIQUE (la diffusion anime
           les faisceaux + cycle de couleurs). Même tilt que .bg-layer —
           la lumière est posée sur le fond. pointer-events:none : hors du
           chemin du drag/sélection, comme .bg-titre. -->
      {#if spotsActif}
        <div class="spots-preview" style={bgTiltStyle + "opacity:" + spotsOpacite + ";"}>
          <div class="spot-ph gauche"></div>
          <div class="spot-ph droite"></div>
        </div>
      {/if}

      <!-- Titre du fond de l'application (intérieur haut/bas) — hors .bg-layer
           pour ne pas être clippé par overflow:hidden. Au-dessus du fond
           (z-index 1), sous les widgets. Gradient de texte reprenant les
           couleurs du cadre unifié. -->
      {#if bgTitreHaut}
        <div class="bg-titre haut" style={bgTitreStyle}>{bgTitreHaut}</div>
      {/if}
      {#if bgTitreBas}
        <div class="bg-titre bas" style={bgTitreStyle}>{bgTitreBas}</div>
      {/if}
      {#if bgTitreGauche}
        <div class="bg-titre gauche" style={bgTitreStyle}>{bgTitreGauche}</div>
      {/if}
      {#if bgTitreDroite}
        <div class="bg-titre droite" style={bgTitreStyle}>{bgTitreDroite}</div>
      {/if}
      {#each $sceneStore.widgets as w (w.id)}
        <WidgetComp {w} {scale} />
      {/each}

      <!-- Poignées resize du widget sélectionné : overlay frère des .widget
           (z 9500, au-dessus des widgets et modules) — jamais masquées par
           un voisin chevauchant, hit-box écran constante. -->
      {#if widgetSelectionne}
        <SelectionOverlay w={widgetSelectionne} {scale} />
      {/if}

      <!-- Modules HUD (scene.modules — horloge / musique / objectifs) :
           couche séparée des widgets, au-dessus d'eux (z 9000 en diffusion),
           drag/resize propres, config via la modale Modules. -->
      {#each MODULE_IDS as id (id)}
        {@const m = $sceneStore.modules?.[id]}
        {#if m?.actif && id !== "spots"}
          <ModuleHud {id} {m} {scale} />
        {/if}
      {/each}

      <!-- Aides contextuelles (presets guidés — dashboard uniquement,
           jamais en diffusion). Auto-masquées : fond → dès qu'un média ou
           une capture OBS est lié ; objectifs → dès que toutes les
           plateformes des goals sont connectées ; cadres → à la première
           ouverture de la modale Cadres. pointer-events:none : les clics
           traversent vers le fond/widgets qu'elles invitent à cliquer. -->
      {#if aideFondVisible || aideCadresVisible}
        <div class="aides-centre">
          {#if aideFondVisible}
            <div class="aide aide-fond">
              <button
                class="aide-fermer"
                title={$t("commun.fermer")}
                aria-label={$t("commun.fermer")}
                onclick={() => fermerAide("aideFond")}
              >✕</button>
              <div class="aide-titre">{$t("canvas.aideFondTitre")}</div>
              <div class="aide-texte">{$t("canvas.aideFond")}</div>
            </div>
          {/if}
          {#if aideCadresVisible}
            <div class="aide aide-cadres">
              <button
                class="aide-fermer"
                title={$t("commun.fermer")}
                aria-label={$t("commun.fermer")}
                onclick={() => fermerAide("aideCadres")}
              >✕</button>
              <div class="aide-titre">{$t("canvas.aideCadresTitre")}</div>
              <div class="aide-texte">{$t("canvas.aideCadres")}</div>
            </div>
          {/if}
        </div>
      {/if}
      {#if aideObjectifsVisible && goalsBox}
        <div
          class="aide aide-objectifs"
          style="left:{goalsBox.cx}px; top:{goalsBox.top - 18}px;"
        >
          <button
            class="aide-fermer"
            title={$t("commun.fermer")}
            aria-label={$t("commun.fermer")}
            onclick={() => fermerAide("aideObjectifs")}
          >✕</button>
          <div class="aide-titre">{$t("canvas.aideObjectifsTitre")}</div>
          <div class="aide-texte">{$t("canvas.aideObjectifs")}</div>
        </div>
      {/if}

      <!-- Cadre unifié — bord du canvas (overlay SVG). Même CadreConfig que
           les widgets. Au-dessus des widgets, pointer-events:none. Visible
           en dashboard ET diffusion. Masquable par bordActif. -->
      {#if cadreBordActif && cadre}
        <div class="cadre-app-overlay">
          <CadreSVG
            style={cadre.style}
            largeur={canvasW}
            hauteur={canvasH}
            strokeWidth={cadre.strokeWidth}
            couleur={cadre.couleur}
            couleurFin={cadre.couleurFin}
            gradientAngle={cadre.gradientAngle}
            animer={cadre.animer === true}
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

{#if fondMenuOpen && ctxWidgetId === null}
  <!-- Menu contextuel du fond de l'application (clic droit canvas hors
       widget). Contenu miroir de la carte fond (CarteEdition, cible "fond") :
       3 branches bgObsSource / !hasFond / média. "Morphing" hardcodé comme
       dans CarteEdition. Les toggles (fit/miroir/lecture/morphing) gardent
       le menu ouvert ; les actions le ferment. -->
  <CtxMenu x={fondMenuX} y={fondMenuY} onclose={fermerFondMenu}>
    {#if bgObsSource}
      <button class="ctx-item" role="menuitem" onclick={ctxObsFond}>{$t("carteEdition.changerCapture")}</button>
      <button class="ctx-item" role="menuitem" onclick={ctxRetirerCapture}>{$t("carteEdition.retirerCapture")}</button>
      <button class="ctx-item" role="menuitem" onclick={ctxTitre}>{hasBgTitre ? $t("carteEdition.titreFondModifier") : $t("carteEdition.titreFondAjouter")}</button>
    {:else if !hasFond}
      <button class="ctx-item" role="menuitem" onclick={ctxImportFond}>{$t("carteEdition.importerFond")}</button>
      <button class="ctx-item" role="menuitem" onclick={ctxObsFond}>{$t("carteEdition.appEnFond")}</button>
    {:else}
      <button class="ctx-item" role="menuitem" onclick={ctxImportFond}>{$t("carteEdition.remplacerFond")}</button>
      <button class="ctx-item" role="menuitem" onclick={ctxObsFond}>{$t("carteEdition.appEnFond")}</button>
      <button class="ctx-item" role="menuitem" onclick={ctxTitre}>{hasBgTitre ? $t("carteEdition.titreFondModifier") : $t("carteEdition.titreFondAjouter")}</button>
      <div class="ctx-sep" role="separator"></div>
      <span class="ctx-label">{$t("carteEdition.affichageFond")}</span>
      <div class="ctx-fit-grid">
        {#each FIT_MODES as mode}
          <button class="ctx-fit-btn" class:active={bgFit === mode} onclick={() => ctxFit(mode)}>{$t("carteEdition.fit." + mode)}</button>
        {/each}
      </div>
      <span class="ctx-label">{$t("carteEdition.miroirFond")}</span>
      <div class="ctx-fit-grid deux">
        <button class="ctx-fit-btn" class:active={bgMiroirH} onclick={() => ctxMiroir("h")}>{$t("carteEdition.miroirH")}</button>
        <button class="ctx-fit-btn" class:active={bgMiroirV} onclick={() => ctxMiroir("v")}>{$t("carteEdition.miroirV")}</button>
      </div>
      <div class="ctx-sep" role="separator"></div>
      {#if bgIsVideo}
        <button class="ctx-item" role="menuitem" onclick={ctxToggleLecture}>{bgPaused ? $t("commun.lecture") : $t("commun.pause")}</button>
      {:else}
        <button class="ctx-item" class:active={$morphMode} role="menuitem" onclick={ctxToggleMorph}>Morphing</button>
      {/if}
      <div class="ctx-sep" role="separator"></div>
      <button class="ctx-item" role="menuitem" onclick={ctxResetTransfo}>{$t("carteEdition.resetTransformation")}</button>
      <button class="ctx-item" role="menuitem" onclick={ctxResetEffets}>{$t("carteEdition.resetEffets")}</button>
      {#if bgAMorphs}
        <button class="ctx-item" role="menuitem" onclick={ctxResetMorphs}>{$t("carteEdition.resetMorphing")}</button>
      {/if}
      <div class="ctx-sep" role="separator"></div>
      {#if confirmDeleteFond}
        <div class="ctx-confirm">
          <span class="ctx-confirm-label">{$t("sceneBar.supprimerConfirm")}</span>
          <div class="ctx-confirm-buttons">
            <button class="ctx-item danger" role="menuitem" onclick={ctxSupprimerConfirm}>{$t("commun.oui")}</button>
            <button class="ctx-item" role="menuitem" onclick={() => (confirmDeleteFond = false)}>{$t("commun.non")}</button>
          </div>
        </div>
      {:else}
        <button class="ctx-item danger" role="menuitem" onclick={() => (confirmDeleteFond = true)}>{$t("carteEdition.supprimerFond")}</button>
      {/if}
    {/if}
  </CtxMenu>
{/if}

{#if fondMenuOpen && ctxWidget}
  <!-- Menu contextuel d'un widget (clic droit dessus). Contenu miroir de la
       carte widget : actions communes d'abord, réglages média si w.media,
       branche média (trou/OBS/flottant) pour les types média, suppression
       via la modale ConfirmDeleteWidgetModal. -->
  <CtxMenu x={fondMenuX} y={fondMenuY} onclose={fermerFondMenu}>
    <button class="ctx-item" role="menuitem" onclick={() => ctxWgTitre(ctxWidget!.id)}>{$t("titreModal.titreWidget")}</button>
    {#if wgType !== "camera"}
      <!-- Caméra exclue : dupliquerWidget refuse (singleton figé) —
           l'entrée serait un no-op silencieux. -->
      <button class="ctx-item" role="menuitem" onclick={() => ctxWgDupliquer(ctxWidget!.id)}>{$t("carteEdition.dupliquerWidget")}</button>
    {/if}
    <button class="ctx-item" role="menuitem" onclick={() => ctxWgPleinEcran(ctxWidget!.id)}>{ctxWidget?.pleinEcran ? $t("carteEdition.pleinEcranOff") : $t("carteEdition.pleinEcran")}</button>
    {#if wgType === "camera"}
      <!-- Œil caméra (même action que le badge de survol) — la caméra
           n'a pas de −/+ (plage réservée z=5000) mais l'œil est un verbe
           du menu comme du badge (règle « 3 surfaces » : aucune action
           exclusive au badge). -->
      <button class="ctx-item" role="menuitem" onclick={() => ctxWgCameraCachee(ctxWidget!.id)}>{ctxWidget?.cameraCachee === true ? $t("widget.cameraAfficher") : $t("widget.cameraMasquer")}</button>
    {:else}
      <!-- Plan (mêmes actions que les boutons -/+ au survol) — caméra
           exclue : plage réservée z=5000 (mettreWidgetPlan est no-op dessus). -->
      <button class="ctx-item" role="menuitem" onclick={() => ctxWgDevant(ctxWidget!.id)}>{$t("carteEdition.premierPlan")}</button>
      <button class="ctx-item" role="menuitem" onclick={() => ctxWgDerriere(ctxWidget!.id)}>{$t("carteEdition.arrierePlan")}</button>
    {/if}
    <div class="ctx-sep" role="separator"></div>
    {#if wgHasMedia}
      <span class="ctx-label">{$t("carteEdition.affichage")}</span>
      <div class="ctx-fit-grid">
        {#each FIT_MODES as mode}
          <button class="ctx-fit-btn" class:active={wgFit === mode} onclick={() => ctxWgFit(ctxWidget!.id, mode)}>{$t("carteEdition.fit." + mode)}</button>
        {/each}
      </div>
      <span class="ctx-label">{$t("carteEdition.miroirMedia")}</span>
      <div class="ctx-fit-grid deux">
        <button class="ctx-fit-btn" class:active={wgMiroirH} onclick={() => ctxWgMiroirMedia(ctxWidget!.id, "h")}>{$t("carteEdition.miroirH")}</button>
        <button class="ctx-fit-btn" class:active={wgMiroirV} onclick={() => ctxWgMiroirMedia(ctxWidget!.id, "v")}>{$t("carteEdition.miroirV")}</button>
      </div>
    {/if}
    <span class="ctx-label">{$t("carteEdition.retournement")}</span>
    <div class="ctx-fit-grid">
      <button class="ctx-fit-btn" onclick={() => ctxWgRetourne(ctxWidget!.id, "h")}>{$t("carteEdition.miroirH")}</button>
      <button class="ctx-fit-btn" onclick={() => ctxWgRetourne(ctxWidget!.id, "v")}>{$t("carteEdition.miroirV")}</button>
      <button class="ctx-fit-btn" onclick={() => ctxWgRetourne(ctxWidget!.id, "reset")}>{$t("carteEdition.reset")}</button>
    </div>
    {#if wgHasMedia}
      <div class="ctx-sep" role="separator"></div>
      {#if wgIsVideo}
        <button class="ctx-item" role="menuitem" onclick={() => ctxWgLecture(ctxWidget!.id)}>{wgPaused ? $t("commun.lecture") : $t("commun.pause")}</button>
      {:else if wgMorphable}
        <button class="ctx-item" class:active={$morphMode} role="menuitem" onclick={ctxWgMorph}>Morphing</button>
      {/if}
      <button class="ctx-item" role="menuitem" onclick={() => ctxWgResetTransfo(ctxWidget!.id)}>{$t("carteEdition.resetTransformation")}</button>
      <button class="ctx-item" role="menuitem" onclick={() => ctxWgResetEffets(ctxWidget!.id)}>{$t("carteEdition.resetEffets")}</button>
      {#if wgHasMorphs}
        <button class="ctx-item" role="menuitem" onclick={() => ctxWgResetMorphs(ctxWidget!.id)}>{$t("carteEdition.resetMorphing")}</button>
      {/if}
    {/if}
    {#if wgType !== "camera" || wgIsBrancheMedia}
      <div class="ctx-sep" role="separator"></div>
      {#if wgType !== "camera"}
        <button class="ctx-item" role="menuitem" onclick={ctxWgImportMedia}>{wgHasMedia ? $t("carteEdition.remplacerMedia") : $t("carteEdition.importerMedia")}</button>
        {#if wgHasMedia}
          <button class="ctx-item" role="menuitem" onclick={() => ctxWgSupprMedia(ctxWidget!.id)}>{$t("carteEdition.supprimerMedia")}</button>
        {/if}
      {/if}
      {#if wgIsBrancheMedia}
        {#if wgHasMedia}
          <button class="ctx-item" role="menuitem" onclick={() => (wgFlottant ? ctxWgTransformerWidget(wgMedia) : ctxWgFlottant(ctxWidget!.id))}>{wgFlottant ? $t("carteEdition.transformerWidget") : $t("carteEdition.transformerFlottant")}</button>
        {/if}
        <button class="ctx-item" class:active={wgIsTrou} role="menuitem" onclick={() => ctxWgTrou(ctxWidget!.id)}>{wgIsTrou ? $t("carteEdition.trouOff") : $t("carteEdition.trouOn")}</button>
        {#if wgIsTrou}
          {#if wgHasObsSource}
            <button class="ctx-item" role="menuitem" onclick={() => ctxWgSupprObsSource(ctxWidget!.id)}>{$t("carteEdition.sourceObsSupprimer")}</button>
          {:else}
            <button class="ctx-item" role="menuitem" onclick={() => ctxWgObsDialog(ctxWidget!.id)}>{$t("carteEdition.sourceObsLier")}</button>
          {/if}
        {/if}
      {/if}
    {/if}
    <div class="ctx-sep" role="separator"></div>
    <button class="ctx-item danger" role="menuitem" onclick={ctxWgSupprimer}>{$t("carteEdition.supprimerWidget")}</button>
  </CtxMenu>
{/if}

{#if obsFondDialogOpen}
  <!-- Capture app/jeu en fond — clic droit → « Mettre une app/jeu en fond »
       ou « Changer l'app/jeu capturé ». -->
  <ObsSourceDialog mode="fond" onClose={() => (obsFondDialogOpen = false)} />
{/if}

{#if obsWidgetDialogId}
  <!-- Source OBS d'un widget trou — clic droit widget → « Lier une source
       OBS ». -->
  <ObsSourceDialog widgetId={obsWidgetDialogId} onClose={() => (obsWidgetDialogId = null)} />
{/if}

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
    /* Conteneur de requête pour le font-size cqw de la pastille
       .msg-pill (.lecture-obs-fond) — recette app.css. */
    container-type: inline-size;
  }
  /* ===== Spots lumineux — aperçu statique (dashboard) ===== */
  /* Couche fond : mêmes règles que .bg-layer (z 0, overflow caché), sans
     pointer-events. Les cônes figés annoncent l'effet animé de diffusion
     (couleurs fluo = contenu de l'effet, pas des tokens UI). */
  .spots-preview {
    position: absolute;
    inset: 0;
    overflow: hidden;
    z-index: 0;
    pointer-events: none;
  }
  .spot-ph {
    position: absolute;
    top: -4%;
    width: 30%;
    height: 120%;
    transform-origin: 50% 0;
    clip-path: polygon(46% 0, 54% 0, 100% 100%, 0 100%);
    filter: blur(8px);
    /* FIXE : pas d'animation côté dashboard — l'aperçu doit être un
       placeholder sans coût, l'animation n'existe qu'en diffusion. */
  }
  .spot-ph::before {
    content: "";
    position: absolute;
    top: -6%;
    left: 38%;
    width: 24%;
    height: 8%;
    border-radius: 50%;
    background: inherit;
    filter: blur(6px);
  }
  .spot-ph.gauche {
    left: 1%;
    transform: rotate(16deg);
    background: linear-gradient(180deg,
      rgba(16, 185, 60, 0.55), rgba(16, 185, 60, 0.18) 55%, transparent 88%);
  }
  .spot-ph.droite {
    right: 1%;
    transform: rotate(-16deg);
    background: linear-gradient(180deg,
      rgba(191, 0, 255, 0.55), rgba(191, 0, 255, 0.16) 55%, transparent 88%);
  }
  /* ===== Titres du fond de l'application ===== */
  /* Positionnés à l'intérieur du canvas : haut/bas centrés horizontalement,
     gauche/droite centrés verticalement en TEXTE VERTICAL (writing-mode).
     Au-dessus du fond (z-index 1), sous les widgets. pointer-events:none
     pour ne pas bloquer le drag/sélection. */
  .bg-titre {
    position: absolute;
    white-space: nowrap;
    pointer-events: none;
    z-index: 1;
    line-height: 1.1;
    font-weight: 400;
    text-align: center;
    /* Contour des lettres (couleur + épaisseur réglables) : inline dans le
       template. paint-order:stroke paint le contour SOUS le gradient de
       remplissage (sinon il mange les glyphes). */
    paint-order: stroke fill;
  }
  .bg-titre.haut,
  .bg-titre.bas {
    left: 50%;
    transform: translateX(-50%);
  }
  .bg-titre.haut { top: 8px; }
  .bg-titre.bas { bottom: 8px; }
  .bg-titre.gauche,
  .bg-titre.droite {
    top: 50%;
    transform: translateY(-50%);
    writing-mode: vertical-rl;
  }
  .bg-titre.gauche { left: 8px; }
  .bg-titre.droite { right: 8px; }
  /* ===== Aides contextuelles (presets guidés) ===== */
  /* Encadré jaune-orangé très visible, dashboard uniquement. pointer-events
     none : les clics traversent vers le fond/widgets que l'aide invite à
     cliquer. z-index au-dessus des widgets (z max 5000) et du cadre de bord. */
  .aide {
    position: absolute;
    pointer-events: none;
    max-width: 620px;
    padding: 16px 26px;
    border-radius: 16px;
    background: rgba(24, 18, 6, 0.9);
    border: 2.5px solid #ffb300;
    box-shadow:
      0 0 30px rgba(255, 170, 0, 0.45),
      inset 0 0 24px rgba(255, 170, 0, 0.07);
    text-align: center;
    font-family: "Fredoka", sans-serif;
    z-index: 9000;
  }
  .aide-titre {
    font-size: 24px;
    font-weight: 700;
    letter-spacing: 0.5px;
    background: linear-gradient(135deg, #ffd54a, #ff8f00);
    -webkit-background-clip: text;
    background-clip: text;
    color: transparent;
  }
  .aide-texte {
    margin-top: 6px;
    font-size: 16px;
    line-height: 1.35;
    color: #ffedb8;
  }
  /* Groupe centré : empile les encadrés centrés (aideFond, aideCadres) —
     chacun se centre seul si l'autre est fermé. Les .aide enfants repassent
     en relative pour que le ✕ se positionne sur son encadré, pas le groupe. */
  .aides-centre {
    position: absolute;
    left: 50%;
    top: 50%;
    transform: translate(-50%, -50%);
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 14px;
    pointer-events: none;
    z-index: 9000;
  }
  .aides-centre .aide {
    position: relative;
  }
  .aide-objectifs {
    transform: translate(-50%, -100%);
  }
  /* Croix de fermeture de l'aide : seul élément cliquable de l'encadré
     (le parent reste pointer-events:none). Petit rond ambre en haut à
     droite, hover lumineux. */
  .aide-fermer {
    position: absolute;
    top: 6px;
    right: 8px;
    pointer-events: auto;
    width: 26px;
    height: 26px;
    padding: 0;
    border-radius: 50%;
    border: 1.5px solid #ffb300;
    background: rgba(255, 179, 0, 0.12);
    color: #ffd54a;
    font-size: 14px;
    font-weight: 700;
    line-height: 1;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .aide-fermer:hover {
    background: rgba(255, 179, 0, 0.3);
    box-shadow: 0 0 10px rgba(255, 170, 0, 0.5);
  }
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
  /* Fond pixelisé (chemin non-morph) : le média reste chargé comme source
     drawImage mais masqué (visibility — jamais display:none, sinon perte de
     naturalWidth/videoWidth). Le canvas overlay est peint par rendreBgPixel. */
  .bg-cachee {
    visibility: hidden;
  }
  .bg-pixel-canvas {
    position: absolute;
    inset: 0;
    display: block;
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
  /* Badge « en lecture dans OBS » (fond) : pastille .msg-pill (recette
     app.css), teinte orange partagée (.lecture-obs-fond, règle globale). */
  /* Overlay cadre unifié : bord extérieur du canvas, au-dessus des widgets. */
  .cadre-app-overlay {
    position: absolute;
    inset: 0;
    pointer-events: none;
    z-index: 100;
  }
  /* Menus contextuels (clic droit fond / widget) : recette .ctx-* GLOBALE
     dans app.css — ne jamais re-copier le bloc ici (même règle que
     .msg-pill). */
</style>
