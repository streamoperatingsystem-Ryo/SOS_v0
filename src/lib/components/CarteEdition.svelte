<script lang="ts">
  // CarteEdition — carte d'édition contextuelle (en haut du panneau latéral).
  // Apparaît quand on clique sur un widget (cible "widget", carte rouge bordeaux),
  // un widget input-viewer (carte jaune) ou sur le fond de l'application
  // (cible "fond", carte orange). Disparaît quand rien n'est sélectionné.
  // Mutuellement exclusive avec l'accordéon (Toolbar). Style : bordure pointillée
  // + glow, palette --dash-* (app.css) — couleurs assombries
  // (--dash-danger = widget, --dash-jaune = input-viewer, --dash-orange = fond).
  import {
    selectedIdStore, sceneStore,
    importMedia, setMediaFit, setMediaZoomLocal, setMediaRotLocal,
    setMediaOffsetXLocal, setMediaOffsetYLocal, setMediaLumLocal,
    setMediaContrasteLocal, setMediaTeinteLocal, setMediaFlouLocal,
    setMediaPixelLocal, resetMediaTransformation, resetMediaEffets,
    clearWidgetMedia, commitScene,
    toggleMediaMiroir, toggleBgMiroir,
    importFond, setBgFit, setBgZoomLocal, setBgRotLocal, setBgOffsetXLocal,
    setBgOffsetYLocal, setBgLumLocal, setBgContrasteLocal, setBgTeinteLocal,
    setBgFlouLocal, setBgPixelLocal, resetFondTransformation, resetFondEffets,
    clearFond, retirerFondCapture,
    setWidgetMediaPaused, setWidgetMediaTime, setWidgetMediaTimeLocal,
    setWidgetMediaLoop,
    setBgPaused, setBgTime, setBgTimeLocal, setBgLoop,
    setWidgetTrou, deleteObsTrouSource, dupliquerWidget, rotateWidgetLocal,
    setWidgetSansCadre, mettreAJourCadre,
    setChatFiltre, setChatStyle, setChatTaillePoliceLocal,
    setSpeedrunPolice, setSpeedrunPoliceLocal, setSpeedrunConfig,
    setInputViewerConfig, setInputViewerConfigLocal, setCameraDevice, setGoalConfig,
    setCameraForme, setCameraBord, setCameraFondVert, setCameraCachee,
    cameraDevicesLive,
    setMusiqueConfig,
    resetMorphsWidget, resetMorphsFond,
    ajouterSlidesMedia, retirerSlideMedia, deplacerSlideMedia,
    setSlidesSecMedia, viderSlidesMedia,
    ajouterSlidesFond, retirerSlideFond, deplacerSlideFond,
    setSlidesSecFond, viderSlidesFond,
  } from "../stores/scene";
  import { morphMode, morphIntensite, morphRayon, morphSens } from "../stores/morph";
  import { videoRegistry } from "../stores/video";
  import { carteEditionCible, confirmDeleteWidget, titreFondModalOpen, cadreModalOpen, speedrunLayoutModalOpen } from "../stores/ui";
  import { type ChatFiltre } from "../stores/chat";
  import {
    phaseTimer, totalSplits, segmentsLss,
    nomJeuLss, nomCategorieLss, estActif, estConnecte, nomProcessus,
    cheminAsl, cheminLss, settingsAsl, erreurSpeedrun,
    chargerAsl, chargerLss, demarrerSpeedrun, arreterSpeedrun, majSettings,
    actionManuelle, sauverPreferencesSpeedrun, exporterSplits,
  } from "../stores/speedrun";
  import { testerMetronome, battementEnCours } from "../stores/metronome";
  import { tauri } from "../tauri";
  import {
    GOAL_METRIQUES, goalSuggestion,
    type GoalPlateforme, type GoalMetrique, type TwitchGoal, type MediaSlide,
  } from "../tauri";
  import { valeursGoals, goalsTwitch } from "../stores/objectifs";
  import { connexions, connecterTwitch } from "../stores/chat";
  import { t, localeTag } from "../stores/langue";
  import { onMount } from "svelte";
  import { get } from "svelte/store";
  import { listen } from "@tauri-apps/api/event";
  import { obsStatus, obsHost, obsPort, obsPassword } from "../stores/obs";
  import { moteurEtat } from "../stores/moteur";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import Gizmo3D from "./Gizmo3D.svelte";
  import PlayerBar from "./PlayerBar.svelte";
  import ObsSourceDialog from "./ObsSourceDialog.svelte";
  import InputBindingModal from "./InputBindingModal.svelte";
  import { POLICES_TITRE } from "../fonts";

  let cible = $derived($carteEditionCible);
  let selectedId = $derived($selectedIdStore);

  // Widget sélectionné (pour lire mediaFit, etc.).
  let selectedWidget = $derived(
    $sceneStore.widgets.find((w) => w.id === selectedId) ?? null
  );

  // Mode natif (lot N3b) : labels « capture » à la place de « source OBS ».
  let natifMode = $derived($moteurEtat.mode === "natif");

  // ===== Détection automatique de la cible =====
  // Si un widget est sélectionné → cible "widget". Sinon → "fond".
  // Le store carteEditionCible est piloté par Canvas (clic fond) et Widget
  // (clic widget) via setCarteEdition(). On synchronise ici : si selectedId
  // change (désélection), on passe à "fond" ou null.
  $effect(() => {
    if (cible === null) return;
    if (selectedId) {
      // Un widget est sélectionné → s'assurer qu'on édite le widget.
      if (cible !== "widget") carteEditionCible.set("widget");
    } else {
      // Pas de widget sélectionné → si on était en mode widget, passer au fond.
      if (cible === "widget") carteEditionCible.set("fond");
    }
  });

  // ===== État déplié (toggle ▼/▶) =====
  let ouverte = $state(true);
  // Auto-déplier à chaque changement de cible (l'utilisateur voit immédiatement
  // les options de ce qu'il vient de cliquer).
  let derniereCible: string | null = null;
  $effect(() => {
    const c = cible;
    if (c !== derniereCible) {
      derniereCible = c;
      ouverte = c !== null;
    }
  });

  // ===== Onglets horizontaux (Contenu / Transformation / Effets / Lecture) =====
  // « Dupliquer » n'est plus un onglet : bouton du pied de carte (toujours
  // accessible quel que soit l'onglet), qui bascule sur Transformation
  // (gizmo + retournement pour orienter la copie).
  let tabWidget = $state<"contenu" | "transformation" | "effets" | "lecture">("contenu");
  let tabFond = $state<"contenu" | "transformation" | "effets" | "lecture">("contenu");
  // Réinitialise à "contenu" quand on change de widget sélectionné — SAUF juste
  // après une duplication (la copie devient sélectionnée : garder l'onglet
  // Transformation pour l'orienter via le gizmo, voir onDupliquer).
  let garderTab = false;
  $effect(() => {
    void selectedId;
    if (garderTab) {
      garderTab = false;
      return;
    }
    tabWidget = "contenu";
  });
  // Onglets fond : reset à "contenu" à chaque (re)ouverture de la carte fond —
  // symétrique au reset de tabWidget à chaque changement de widget.
  $effect(() => {
    void cible;
    if (cible === "fond") tabFond = "contenu";
  });

  // ===== Type du widget sélectionné =====
  let selectedType = $derived(selectedWidget?.type ?? "media");
  let isChatWidget = $derived(selectedType === "chat");
  let isCameraWidget = $derived(selectedType === "camera");
  let isInputViewerWidget = $derived(selectedType === "input-viewer");

  // Classe CSS de la carte : dérivée de la cible + type de widget. Un widget
  // input-viewer obtient sa propre carte jaune (cible-input-viewer) au lieu de
  // la carte widget rouge. Les autres widgets → cible-widget (rouge), le fond
  // → cible-fond (orange).
  let carteClasse = $derived(
    cible === "widget" && isInputViewerWidget ? "cible-input-viewer" : `cible-${cible}`
  );
  let isSpeedrunWidget = $derived(selectedType === "speedrun");
  let isGoalWidget = $derived(selectedType === "goal");
  let isMusiqueWidget = $derived(selectedType === "musique");
  let musiqueModeVal = $derived(selectedWidget?.musiqueMode ?? "player");
  let isMediaWidget = $derived(selectedType === "media");
  let chatFiltreVal = $derived(selectedWidget?.chatFiltre ?? "unifie");
  let chatStyleVal = $derived(selectedWidget?.chatStyle ?? "compact");
  let chatTaille = $derived(selectedWidget?.taillePolice ?? 16);

  // ===== Widget goal : config dérivée + valeur live =====
  const GOAL_PLATEFORMES: { id: GoalPlateforme; label: string }[] = [
    { id: "twitch", label: "Twitch" },
    { id: "youtube", label: "YouTube" },
    { id: "kick", label: "Kick" },
    { id: "tiktok", label: "TikTok" },
  ];
  let goalPlat = $derived<GoalPlateforme>(selectedWidget?.goalPlateforme ?? "twitch");
  let goalMet = $derived<GoalMetrique>(selectedWidget?.goalMetrique ?? "followers");
  let goalCibleVal = $derived(selectedWidget?.goalCible ?? 0);
  // Valeur live poussée par les events communaute:update / viewers:update.
  let goalActuel = $derived(
    goalPlat ? $valeursGoals[goalPlat]?.[goalMet] : undefined
  );
  let goalSuggere = $derived(
    goalActuel === undefined ? undefined : goalSuggestion(goalPlat, goalMet, goalActuel)
  );

  function onGoalPlateforme(p: GoalPlateforme): void {
    if (!selectedId) return;
    // Métrique absente de la nouvelle plateforme → bascule sur la 1re dispo
    // (pas de valeur fabriquée : seules les métriques réellement mesurées).
    // Hors Twitch → goalId effacé : les Creator Goals déclarés n'existent
    // que chez Twitch.
    const patch: Parameters<typeof setGoalConfig>[1] = { goalPlateforme: p };
    if (p !== "twitch") {
      patch.goalId = undefined;
    }
    if (!GOAL_METRIQUES[p].includes(goalMet)) {
      patch.goalMetrique = GOAL_METRIQUES[p][0];
    }
    void setGoalConfig(selectedId, patch);
  }

  // ===== Widget goal : Creator Goals Twitch déclarés (mode lié) =====
  // Liste relue à l'ouverture de la carte (zéro persistance — GET /goals).
  // Les valeurs live (EventSub + poll viewers.rs) arrivent via goalsTwitch :
  // l'affichage prend la plus fraîche des deux sources.
  let goalsTwitchListe = $state<TwitchGoal[] | null>(null);
  let goalsErreur = $state<string | null>(null);
  let goalsChargement = $state(false);
  let goalLie = $derived(selectedWidget?.goalId);
  let twitchConnecte = $derived($connexions.twitch);

  async function chargerGoalsTwitch(): Promise<void> {
    goalsChargement = true;
    goalsErreur = null;
    try {
      goalsTwitchListe = await tauri.twitchGoalsLister();
    } catch (e) {
      goalsTwitchListe = null;
      goalsErreur = typeof e === "string" ? e : String(e);
    } finally {
      goalsChargement = false;
    }
  }

  // Auto-fetch : carte d'un widget goal + plateforme twitch + connecté →
  // GET /goals ; re-fetch au retour de la connexion (false → true).
  $effect(() => {
    if (isGoalWidget && goalPlat === "twitch" && twitchConnecte) {
      void chargerGoalsTwitch();
    } else {
      goalsTwitchListe = null;
      goalsErreur = null;
    }
  });

  // Clic sur un goal : lier ; re-clic sur le goal déjà lié : délier (toggle).
  function onGoalLier(g: TwitchGoal): void {
    if (!selectedId) return;
    if (goalLie === g.id) {
      void setGoalConfig(selectedId, { goalId: undefined });
    } else {
      void setGoalConfig(selectedId, { goalPlateforme: "twitch", goalId: g.id });
    }
  }

  function onGoalCibleChange(e: Event): void {
    if (!selectedId) return;
    const v = Math.max(1, Math.round(Number((e.target as HTMLInputElement).value) || 0));
    void setGoalConfig(selectedId, { goalCible: v });
  }

  // ===== Label du widget pour l'en-tête =====
  // Affiche le type lisible du widget (ex: "Splitter", "Chat", "Caméra") au
  // lieu de l'ID technique. Si l'utilisateur a défini un titre personnalisé
  // (champ titre du widget), on l'utilise en priorité. Pour les widgets média,
  // on ajoute le nom du fichier importé si disponible (mediaNom).
  const TYPES_WIDGET = [
    "media", "chat", "camera", "input-viewer", "speedrun", "goal",
    "musique",
  ];
  let nomsTypes = $derived.by(() => {
    const tr = $t;
    return Object.fromEntries(
      TYPES_WIDGET.map((ty) => [ty, tr("carteEdition.type." + ty)])
    ) as Record<string, string>;
  });
  let widgetLabel = $derived.by(() => {
    // Titre perso prioritaire.
    if (selectedWidget?.titre && selectedWidget.titre.length > 0) {
      return selectedWidget.titre;
    }
    const typeLabel = nomsTypes[selectedWidget?.type ?? "media"] ?? $t("commun.widget");
    // Pour les widgets média avec un nom de fichier, afficher le nom.
    if (selectedWidget?.type === "media" && selectedWidget?.mediaNom) {
      return selectedWidget.mediaNom;
    }
    return typeLabel;
  });

  // ===== Valeurs dérivées du widget sélectionné =====
  let currentFit = $derived(selectedWidget?.mediaFit ?? "ajuster");
  let currentZoom = $derived(selectedWidget?.mediaZoom ?? 1);
  let currentRot = $derived(selectedWidget?.mediaRot ?? 0);
  let currentOffsetX = $derived(selectedWidget?.mediaOffsetX ?? 0);
  let currentOffsetY = $derived(selectedWidget?.mediaOffsetY ?? 0);
  let currentLum = $derived(selectedWidget?.mediaLum ?? 0);
  let currentContraste = $derived(selectedWidget?.mediaContraste ?? 0);
  let currentTeinte = $derived(selectedWidget?.mediaTeinte ?? 0);
  let currentFlou = $derived(selectedWidget?.mediaFlou ?? 0);
  let currentPixel = $derived(selectedWidget?.mediaPixel ?? 0);
  let currentMiroirH = $derived(selectedWidget?.mediaMiroirH === true);
  let currentMiroirV = $derived(selectedWidget?.mediaMiroirV === true);
  let selectedKind = $derived(selectedWidget?.kind ?? "image");
  let selectedTrou = $derived(selectedWidget?.trou === true);
  let selectedObsSource = $derived(selectedWidget?.obsSource ?? null);
  let widgetMediaPaused = $derived(selectedWidget?.mediaPaused ?? true);
  let widgetMediaTime = $derived(selectedWidget?.mediaTime ?? 0);
  let widgetLoopA = $derived(selectedWidget?.mediaLoopA ?? null);
  let widgetLoopB = $derived(selectedWidget?.mediaLoopB ?? null);
  let widgetVideoEl = $derived(selectedId ? $videoRegistry.get(selectedId) : undefined);

  // ===== Valeurs dérivées du fond =====
  let bgMedia = $derived($sceneStore.bgMedia ?? "");
  let bgKind = $derived($sceneStore.bgKind ?? "image");
  let bgFit = $derived($sceneStore.bgFit ?? "remplir");
  let bgZoom = $derived($sceneStore.bgZoom ?? 1);
  let bgRot = $derived($sceneStore.bgRot ?? 0);
  let bgOffsetX = $derived($sceneStore.bgOffsetX ?? 0);
  let bgOffsetY = $derived($sceneStore.bgOffsetY ?? 0);
  let bgLum = $derived($sceneStore.bgLum ?? 0);
  let bgContraste = $derived($sceneStore.bgContraste ?? 0);
  let bgTeinte = $derived($sceneStore.bgTeinte ?? 0);
  let bgFlou = $derived($sceneStore.bgFlou ?? 0);
  let bgPixel = $derived($sceneStore.bgPixel ?? 0);
  let bgMiroirH = $derived($sceneStore.bgMiroirH === true);
  let bgMiroirV = $derived($sceneStore.bgMiroirV === true);
  // Slides du carrousel de fond — liste non vide = prioritaire sur bgMedia.
  let bgSlides = $derived($sceneStore.bgSlides ?? []);
  let bgSlidesSec = $derived($sceneStore.bgSlidesSec ?? 5);
  let hasFond = $derived(bgMedia.length > 0 || bgSlides.length > 0);
  // Fond = capture OBS (app/jeu plein canvas) : PRIORITAIRE sur le média —
  // bgMedia reste stocké mais n'est pas peint tant que la capture est active.
  let bgObsSource = $derived($sceneStore.bgObsSource ?? null);
  let hasBgTitre = $derived(($sceneStore.bgTitre ?? "").length > 0);
  let fondVideoEl = $derived($videoRegistry.get("fond"));
  let bgPaused = $derived($sceneStore.bgPaused ?? true);
  let bgTime = $derived($sceneStore.bgTime ?? 0);
  let bgLoopA = $derived($sceneStore.bgLoopA ?? null);
  let bgLoopB = $derived($sceneStore.bgLoopB ?? null);

  // ===== Carrousel d'images (mediaSlides widget / bgSlides fond) =====
  // Liste non vide = carrousel actif (prioritaire sur le média unique, qui
  // reste stocké). Images uniquement — fondu enchaîné côté diffusion.
  let widgetSlides = $derived(selectedWidget?.mediaSlides ?? []);
  let widgetSlidesSec = $derived(selectedWidget?.mediaSlidesSec ?? 5);
  // Brouillons d'intervalle pendant le drag du slider — commit au change,
  // même pattern que les sliders de transformation.
  let widgetSlidesSecBrouillon = $state<number | null>(null);
  let bgSlidesSecBrouillon = $state<number | null>(null);
  let widgetSlidesSecVal = $derived(widgetSlidesSecBrouillon ?? widgetSlidesSec);
  let bgSlidesSecVal = $derived(bgSlidesSecBrouillon ?? bgSlidesSec);

  function onAjouterSlidesMedia(): void { void ajouterSlidesMedia(); }
  function onAjouterSlidesFond(): void { void ajouterSlidesFond(); }
  function onWidgetSlidesSecInput(e: Event): void {
    widgetSlidesSecBrouillon = +(e.target as HTMLInputElement).value;
  }
  function onWidgetSlidesSecChange(): void {
    if (widgetSlidesSecBrouillon !== null) void setSlidesSecMedia(widgetSlidesSecBrouillon);
    widgetSlidesSecBrouillon = null;
  }
  function onBgSlidesSecInput(e: Event): void {
    bgSlidesSecBrouillon = +(e.target as HTMLInputElement).value;
  }
  function onBgSlidesSecChange(): void {
    if (bgSlidesSecBrouillon !== null) void setSlidesSecFond(bgSlidesSecBrouillon);
    bgSlidesSecBrouillon = null;
  }

  // ===== Cadres SVG =====
  // Toggle par widget (sansCadre — opt-out par instance, tous types) +
  // toggle du bord du canvas (bordActif sur la CadreConfig unifiée) +
  // bouton ouvrant la modale Cadres pour le reste des réglages.
  let cadreSceneActif = $derived(
    !!$sceneStore.cadreWidget?.actif && ($sceneStore.cadreWidget?.strokeWidth ?? 0) > 0
  );
  let cadreBordActif = $derived($sceneStore.cadreWidget?.bordActif ?? true);
  // Cadre sur CE widget = !sansCadre.
  let widgetAvecCadre = $derived(selectedWidget?.sansCadre !== true);
  async function onToggleCadreWidget() {
    if (!selectedId) return;
    await setWidgetSansCadre(selectedId, widgetAvecCadre);
  }
  async function onToggleCadreBord() { await mettreAJourCadre({ bordActif: !cadreBordActif }); }
  function onOuvrirCadres() { cadreModalOpen.set(true); }

  // ===== Modes d'affichage (fit) =====
  const FIT_MODES = ["ajuster", "remplir", "etendre", "etirer", "centrer", "vignette"];
  let nomsFit = $derived.by(() => {
    const tr = $t;
    return Object.fromEntries(
      FIT_MODES.map((m) => [m, tr("carteEdition.fit." + m)])
    ) as Record<string, string>;
  });
  function labelFit(mode: string): string { return nomsFit[mode] ?? mode; }

  // ===== Handlers widget =====
  function onFitClick(fit: string) { if (selectedId) setMediaFit(selectedId, fit); }
  function onZoomInput(e: Event) { if (selectedId) setMediaZoomLocal(selectedId, parseFloat((e.target as HTMLInputElement).value)); }
  async function onZoomChange() { await commitScene(); }
  function onRotInput(e: Event) { if (selectedId) setMediaRotLocal(selectedId, parseFloat((e.target as HTMLInputElement).value)); }
  async function onRotChange() { await commitScene(); }
  function onOffsetXInput(e: Event) { if (selectedId) setMediaOffsetXLocal(selectedId, parseFloat((e.target as HTMLInputElement).value)); }
  function onOffsetYInput(e: Event) { if (selectedId) setMediaOffsetYLocal(selectedId, parseFloat((e.target as HTMLInputElement).value)); }
  async function onOffsetChange() { await commitScene(); }
  function onLumInput(e: Event) { if (selectedId) setMediaLumLocal(selectedId, parseFloat((e.target as HTMLInputElement).value)); }
  function onContrasteInput(e: Event) { if (selectedId) setMediaContrasteLocal(selectedId, parseFloat((e.target as HTMLInputElement).value)); }
  function onTeinteInput(e: Event) { if (selectedId) setMediaTeinteLocal(selectedId, parseFloat((e.target as HTMLInputElement).value)); }
  function onFlouInput(e: Event) { if (selectedId) setMediaFlouLocal(selectedId, parseFloat((e.target as HTMLInputElement).value)); }
  function onPixelInput(e: Event) { if (selectedId) setMediaPixelLocal(selectedId, parseFloat((e.target as HTMLInputElement).value)); }
  async function onEffetChange() { await commitScene(); }
  // Miroir média : toggle discret (commit dans le store, pas de slider).
  async function onMiroirMedia(sens: "h" | "v") { if (selectedId) await toggleMediaMiroir(selectedId, sens); }
  async function onResetMediaTransformation() { if (selectedId) await resetMediaTransformation(selectedId); }
  async function onResetMediaEffets() { if (selectedId) await resetMediaEffets(selectedId); }
  async function onClearMedia() { if (selectedId) await clearWidgetMedia(selectedId); }
  function onWidgetTogglePlay() { if (selectedId) setWidgetMediaPaused(selectedId, !widgetMediaPaused); }
  function onWidgetSeek(t: number) { if (selectedId) setWidgetMediaTime(selectedId, t); }
  // Pendant le drag du slider : état local seul (la vignette suit) ; le
  // commit (snapshot :4321) part au release via onSeek — pas 1/frame.
  function onWidgetSeekLive(t: number) { if (selectedId) setWidgetMediaTimeLocal(selectedId, t); }
  function onWidgetLoopChange(a: number | null, b: number | null) {
    if (selectedId) void setWidgetMediaLoop(selectedId, a, b);
  }
  function onToggleTrou() { if (selectedId) setWidgetTrou(selectedId, !selectedTrou); }
  function onSupprimerClick() { confirmDeleteWidget.set(true); }

  // ===== Dupliquer (bouton du pied de carte) =====
  // La copie devient le widget sélectionné ; on bascule sur l'onglet
  // Transformation (gizmo pleine amplitude + retournement exact) pour
  // orienter la copie — garderTab bloque le reset d'onglet pendant que
  // la sélection passe à la copie.
  async function onDupliquer() {
    if (!selectedId) return;
    garderTab = true;
    const newId = await dupliquerWidget(selectedId);
    if (!newId) garderTab = false;
    else tabWidget = "transformation";
  }
  async function onMiroir(sens: "h" | "v" | "reset") {
    if (!selectedId) return;
    rotateWidgetLocal(selectedId, sens === "v" ? 180 : 0, sens === "h" ? 180 : 0);
    await commitScene();
  }

  // ===== Handlers fond =====
  function onBgFitClick(fit: string) { setBgFit(fit); }
  function onBgZoomInput(e: Event) { setBgZoomLocal(parseFloat((e.target as HTMLInputElement).value)); }
  async function onBgZoomChange() { await commitScene(); }
  function onBgRotInput(e: Event) { setBgRotLocal(parseFloat((e.target as HTMLInputElement).value)); }
  async function onBgRotChange() { await commitScene(); }
  function onBgOffsetXInput(e: Event) { setBgOffsetXLocal(parseFloat((e.target as HTMLInputElement).value)); }
  function onBgOffsetYInput(e: Event) { setBgOffsetYLocal(parseFloat((e.target as HTMLInputElement).value)); }
  async function onBgOffsetChange() { await commitScene(); }
  function onBgLumInput(e: Event) { setBgLumLocal(parseFloat((e.target as HTMLInputElement).value)); }
  function onBgContrasteInput(e: Event) { setBgContrasteLocal(parseFloat((e.target as HTMLInputElement).value)); }
  function onBgTeinteInput(e: Event) { setBgTeinteLocal(parseFloat((e.target as HTMLInputElement).value)); }
  function onBgFlouInput(e: Event) { setBgFlouLocal(parseFloat((e.target as HTMLInputElement).value)); }
  function onBgPixelInput(e: Event) { setBgPixelLocal(parseFloat((e.target as HTMLInputElement).value)); }
  async function onBgEffetChange() { await commitScene(); }
  // Miroir fond : toggle discret (commit dans le store).
  async function onBgMiroir(sens: "h" | "v") { await toggleBgMiroir(sens); }
  async function onBgReset() { await resetFondTransformation(); }
  async function onBgResetEffets() { await resetFondEffets(); }
  async function onResetMorphsFond() { await resetMorphsFond(); }
  function onBgTogglePlay() { setBgPaused(!bgPaused); }
  function onBgSeek(t: number) { setBgTime(t); }
  // Idem widget : drag = local, release = commit.
  function onBgSeekLive(t: number) { setBgTimeLocal(t); }
  function onBgLoopChange(a: number | null, b: number | null) {
    void setBgLoop(a, b);
  }

  // ===== Chat widget =====
  const FILTRES: ChatFiltre[] = ["unifie", "twitch", "youtube", "kick", "tiktok"];
  let nomsFiltres = $derived.by(() => {
    const tr = $t;
    return Object.fromEntries(
      FILTRES.map((f) => [f, tr("carteEdition.filtre." + f)])
    ) as Record<string, string>;
  });
  function onFiltreClick(f: ChatFiltre) { if (selectedId) setChatFiltre(selectedId, f); }
  // Mise en forme des messages : "compact" (joints, zébrage) | "bulles"
  // (détachés, cadre SVG de scène par message — comme un widget objectif).
  const CHAT_STYLES = ["compact", "bulles"] as const;
  let nomsChatStyles = $derived.by(() => {
    const tr = $t;
    return Object.fromEntries(
      CHAT_STYLES.map((s) => [s, tr("carteEdition.chatStyle." + s)])
    ) as Record<string, string>;
  });
  function onChatStyleClick(s: string) { if (selectedId) setChatStyle(selectedId, s); }
  // Slider : oninput = état local (zéro commit par tick), onchange = commit.
  function onTailleInput(e: Event) { if (selectedId) setChatTaillePoliceLocal(selectedId, parseFloat((e.target as HTMLInputElement).value)); }
  async function onTailleChange() { await commitScene(); }

  // ===== Pop-out chat =====
  let popoutOpen = $state(false);
  // ===== Pop-out speedrun (même pattern ; event distinct pour ne pas
  // réinitialiser le bouton de l'autre pop-out) =====
  let srPopoutOpen = $state(false);
  onMount(async () => {
    await listen("popout-closed", () => { popoutOpen = false; });
    await listen("speedrun-popout-closed", () => { srPopoutOpen = false; });
  });
  async function onDetacher() {
    try { popoutOpen = await tauri.chatPopoutToggle(); } catch {}
  }
  async function onSrDetacher() {
    try { srPopoutOpen = await tauri.speedrunPopoutToggle(); } catch {}
  }

  // ===== Widget speedrun =====
  let srPoliceVal = $derived(selectedWidget?.srPolice ?? "Rajdhani");
  let srTailleVal = $derived(selectedWidget?.srTaillePolice ?? 27);
  let srLayoutVal = $derived(selectedWidget?.srLayout ?? "auto");
  function onSrLayoutChange(e: Event) { if (!selectedId) return; setSpeedrunConfig(selectedId, { srLayout: (e.target as HTMLSelectElement).value }); }
  function onSrPoliceChange(e: Event) { if (!selectedId) return; const p = (e.target as HTMLSelectElement).value; setSpeedrunPolice(selectedId, p, srTailleVal); }
  function onSrTailleInput(e: Event) { if (!selectedId) return; setSpeedrunPoliceLocal(selectedId, srPoliceVal, parseFloat((e.target as HTMLInputElement).value)); }
  async function onSrTailleChange() { await commitScene(); }

  let srStartActif = $state(true);
  let srSplitActif = $state(true);
  let srResetActif = $state(true);
  $effect(() => {
    srStartActif = $settingsAsl.start;
    srSplitActif = $settingsAsl.split;
    srResetActif = $settingsAsl.reset;
  });

  async function onSpeedrunChoisirAsl() {
    if (!selectedId) return;
    const path = await openDialog({ filters: [{ name: "Auto Split Language", extensions: ["asl"] }], multiple: false });
    if (path && typeof path === "string") {
      await chargerAsl(path);
      await setSpeedrunConfig(selectedId, { speedrunCheminAsl: path });
      await sauverPreferencesSpeedrun(path, $cheminLss, $settingsAsl);
    }
  }
  async function onSpeedrunChoisirLss() {
    if (!selectedId) return;
    const path = await openDialog({ filters: [{ name: "LiveSplit Splits", extensions: ["lss"] }], multiple: false });
    if (path && typeof path === "string") {
      await chargerLss(path);
      await setSpeedrunConfig(selectedId, { speedrunCheminLss: path });
      await sauverPreferencesSpeedrun($cheminAsl, path, $settingsAsl);
    }
  }
  async function onSpeedrunDemarrer() {
    const nb = $totalSplits > 0 ? $totalSplits : ($segmentsLss.length || 1);
    await demarrerSpeedrun(nb);
  }
  async function onSpeedrunArreter() { await arreterSpeedrun(); }
  async function onSpeedrunToggleStart() {
    srStartActif = !srStartActif;
    await majSettings(srStartActif, srSplitActif, srResetActif);
    if (selectedId) await setSpeedrunConfig(selectedId, { speedrunSettings: { start: srStartActif, split: srSplitActif, reset: srResetActif } });
    await sauverPreferencesSpeedrun($cheminAsl, $cheminLss, { start: srStartActif, split: srSplitActif, reset: srResetActif });
  }
  async function onSpeedrunToggleSplit() {
    srSplitActif = !srSplitActif;
    await majSettings(srStartActif, srSplitActif, srResetActif);
    if (selectedId) await setSpeedrunConfig(selectedId, { speedrunSettings: { start: srStartActif, split: srSplitActif, reset: srResetActif } });
    await sauverPreferencesSpeedrun($cheminAsl, $cheminLss, { start: srStartActif, split: srSplitActif, reset: srResetActif });
  }
  async function onSpeedrunToggleReset() {
    srResetActif = !srResetActif;
    await majSettings(srStartActif, srSplitActif, srResetActif);
    if (selectedId) await setSpeedrunConfig(selectedId, { speedrunSettings: { start: srStartActif, split: srSplitActif, reset: srResetActif } });
    await sauverPreferencesSpeedrun($cheminAsl, $cheminLss, { start: srStartActif, split: srSplitActif, reset: srResetActif });
  }

  // ===== Métronome du splitter (dashboard seul) =====
  // Toggle persisté sur le widget (même rangée que les autos) ; sliders en
  // copies locales pendant le geste, commit au change (pattern srTaille).
  // Volume stocké 0-1 sur le widget, affiché 0-100 % côté slider.
  let srMetroActifVal = $derived(selectedWidget?.srMetronomeActif === true);
  let srMetroBpmVal = $state(120);
  let srMetroVolPct = $state(50);
  $effect(() => {
    srMetroBpmVal =
      typeof selectedWidget?.srMetronomeBpm === "number" ? selectedWidget.srMetronomeBpm : 120;
    srMetroVolPct =
      typeof selectedWidget?.srMetronomeVolume === "number"
        ? Math.round(selectedWidget.srMetronomeVolume * 100)
        : 50;
  });
  function onSrMetroToggle() {
    if (!selectedId) return;
    setSpeedrunConfig(selectedId, { srMetronomeActif: !srMetroActifVal });
  }
  function onSrMetroBpmInput(e: Event) {
    srMetroBpmVal = Math.round(parseFloat((e.target as HTMLInputElement).value));
  }
  async function onSrMetroBpmChange() {
    if (!selectedId) return;
    await setSpeedrunConfig(selectedId, {
      srMetronomeBpm: Math.max(1, Math.min(400, srMetroBpmVal)),
    });
  }
  function onSrMetroVolInput(e: Event) {
    srMetroVolPct = Math.round(parseFloat((e.target as HTMLInputElement).value));
  }
  async function onSrMetroVolChange() {
    if (!selectedId) return;
    await setSpeedrunConfig(selectedId, { srMetronomeVolume: srMetroVolPct / 100 });
  }
  function onSrMetroTest() {
    testerMetronome(Math.max(1, Math.min(400, srMetroBpmVal)), srMetroVolPct / 100);
  }

  // ===== Widget input viewer =====
  let inputModeVal = $derived(selectedWidget?.inputMode ?? "clavier");
  let inputLayoutVal = $derived(selectedWidget?.inputLayout ?? "azerty");
  let inputCouleurVal = $derived(selectedWidget?.inputCouleur ?? "#8b5cf6");
  let inputSkinVal = $derived(selectedWidget?.inputSkin ?? "xbox");
  let bindingModalOpen = $state(false);
  let captureActive = $state(false);
  // Relit l'état RÉEL côté Rust à chaque changement de widget sélectionné /
  // affichage de la carte input-viewer : la capture peut avoir été coupée
  // automatiquement (syncInputViewerCapture au changement de scène ou à la
  // suppression du dernier widget) pendant que la carte restait montée.
  $effect(() => {
    void selectedId;
    void isInputViewerWidget;
    tauri.inputViewerEtatActif().then((a) => (captureActive = a)).catch(() => {});
  });
  // Bascule puis RELIT l'état réel (pas de toggle optimiste : un échec
  // d'installation du hook souris repasse actif=false côté Rust — afficher
  // l'état lu évite un bouton « ON » mensonger).
  async function toggleCaptureInput() {
    try {
      await tauri.inputViewerSetActif(!captureActive);
      captureActive = await tauri.inputViewerEtatActif();
    } catch {}
  }

  // ===== Widget caméra =====
  let cameraDeviceId = $derived(selectedWidget?.cameraDeviceId ?? null);
  let cameraDevices = $state<string[]>([]);
  // Re-déclenchée par $cameraDevicesLive : la tâche Rust de hot-plug émet
  // "camera:devices" à chaque branchement/débranchement → le sélecteur se
  // rafraîchit sans rouvrir la carte. Quand OBS est connecté, union avec sa
  // liste dshow (autoritaire : couvre les caméras virtuelles qui ne
  // s'enregistrent pas en PnP).
  $effect(() => {
    if (!isCameraWidget) { cameraDevices = []; return; }
    const live = $cameraDevicesLive;
    const obsOk = $obsStatus === "connected";
    let cancelled = false;
    (async () => {
      let liste: string[];
      try {
        liste = await tauri.pcEnumerateCameras();
      } catch {
        liste = live;
      }
      if (obsOk) {
        try {
          const obs = await tauri.obsEnumerateTargets(
            get(obsHost), parseInt(get(obsPort), 10) || 4455,
            get(obsPassword), "camera"
          );
          liste = [...new Set([...liste, ...obs])];
        } catch {
        }
      }
      if (!cancelled) cameraDevices = liste;
    })();
    return () => { cancelled = true; };
  });
  // Color picker : oninput continu = état local, onchange (fermeture) = commit.
  async function onInputCouleurChange() { await commitScene(); }
  function onCameraDeviceChange(e: Event) {
    if (!selectedId) return;
    const val = (e.target as HTMLSelectElement).value;
    setCameraDevice(selectedId, val === "" ? null : val);
  }

  // ===== Forme caméra dédiée (cameraForme) + fond vert virtuel =====
  // "cadre" = forme du cadre de scène (historique), "cercle" = cercle
  // inscrit, "carre" = rect — découplées du cadre de scène et de sansCadre.
  let cameraFormeVal = $derived(
    selectedWidget?.cameraForme === "cercle" || selectedWidget?.cameraForme === "carre"
      ? selectedWidget.cameraForme
      : "cadre"
  );
  let cameraBordVal = $derived(selectedWidget?.cameraBord === true);
  function onCameraFormeClick(f: "cercle" | "carre" | "cadre") {
    if (selectedId) void setCameraForme(selectedId, f);
  }
  function onToggleCameraBord() {
    if (selectedId) void setCameraBord(selectedId, !cameraBordVal);
  }
  // Caméra masquée en diffusion (cameraCachee — mêmes effets que le bouton
  // œil au survol : items OBS désactivés + widget ignoré par diffusion.html).
  // La carte est la surface A de la règle « 3 surfaces » : l'action reste
  // accessible même si un module HUD recouvre le widget sur le canvas.
  let cameraCacheeVal = $derived(selectedWidget?.cameraCachee === true);
  function onToggleCameraCachee() {
    if (selectedId) void setCameraCachee(selectedId, !cameraCacheeVal);
  }
  // Fond vert virtuel : le flag est persisté même si l'application OBS
  // échoue (OBS offline) → auto-guérison au prochain scene_sync_captures.
  // Plugin absent → le Rust l'INSTALLE automatiquement (téléchargement
  // GitHub ~50 Mo + extraction per-user) → hint « redémarre OBS ».
  // L'erreur s'affiche en hint rouge sous le toggle.
  let cameraFondVertVal = $derived(selectedWidget?.cameraFondVert === true);
  let fondVertErreur = $state("");
  let fondVertInfo = $state("");
  let fondVertEnCours = $state(false);
  async function onToggleCameraFondVert() {
    if (!selectedId || fondVertEnCours) return;
    fondVertErreur = "";
    fondVertInfo = "";
    fondVertEnCours = true;
    const r = await setCameraFondVert(selectedId, !cameraFondVertVal);
    fondVertEnCours = false;
    if (r.erreur) {
      fondVertErreur = r.erreur.startsWith("PLUGIN_ABSENT")
        ? $t("carteEdition.fondVertPluginAbsent")
        : r.erreur;
    } else if (r.statut === "redemarrer-obs") {
      fondVertInfo = $t("carteEdition.fondVertInstalle");
    }
  }

  // ===== Source OBS sous SOS (trou) =====
  let obsDialogOpen = $state(false);
  function onOpenObsDialog() { if (!selectedId || !selectedTrou) return; obsDialogOpen = true; }
  function onCloseObsDialog() { obsDialogOpen = false; }
  async function onDeleteObsSource() { if (!selectedId) return; await deleteObsTrouSource(selectedId); }

  // ===== Fond de scène = capture OBS (app/jeu plein canvas) =====
  // ObsSourceDialog mode "fond" → lierFondCapture/retirerFondCapture (store).
  let obsFondDialogOpen = $state(false);

  function onMorphKeydown(e: KeyboardEvent) {
    if (e.key !== "Escape" || !$morphMode) return;
    const t = e.target as HTMLElement;
    if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.isContentEditable)) return;
    morphMode.set(false);
  }
</script>

<svelte:window onkeydown={onMorphKeydown} />

{#if cible !== null}
  <div class="carte-edition {carteClasse}" class:ouverte>
    <!-- En-tête cliquable (toggle ▼/▶) + bouton fermer séparé -->
    <div class="carte-entete-row">
      <button type="button" class="carte-entete" onclick={() => (ouverte = !ouverte)} aria-expanded={ouverte} title={cible === "widget" && selectedId ? selectedId : undefined}>
        <span class="carte-fleche">{ouverte ? "▼" : "▶"}</span>
        <span class="carte-titre">
          {#if cible === "widget"}
            {widgetLabel}
          {:else}
            {$t("carteEdition.arrierePlanTitre")}
          {/if}
        </span>
      </button>
      <button type="button" class="carte-fermer" onclick={() => carteEditionCible.set(null)} title={$t("commun.fermer")} aria-label={$t("carteEdition.fermerAria")}>✕</button>
    </div>

    {#if ouverte}
      <div class="carte-corps">
        {#if cible === "widget"}
          <!-- ====== ÉDITION WIDGET ====== -->
          {#if !selectedId}
            <p class="hint">{$t("carteEdition.aucunWidget")}</p>
          {:else if isCameraWidget}
            <!-- Widget caméra : pas d'onglets -->
            <div class="section-titre">{$t("carteEdition.type.camera")}</div>
            <div class="field">
              <span class="field-label">{$t("carteEdition.deviceCamera")}</span>
              <select value={cameraDeviceId ?? ""} onchange={onCameraDeviceChange}>
                <option value="">{$t("carteEdition.deviceDefaut")}</option>
                {#each cameraDevices as dev}
                  <option value={dev}>{dev}</option>
                {/each}
              </select>
            </div>
            {#if cameraDevices.length === 0}
              <p class="hint">{$t("carteEdition.aucuneCamera")}</p>
              <p class="hint">{$t("carteEdition.hintAndroidCam")}</p>
            {/if}
            <!-- Forme dédiée caméra (cameraForme) — découplée du cadre de
                 scène : cercle inscrit / carré / forme du cadre. -->
            <div class="section-titre">{$t("carteEdition.sectionForme")}</div>
            <div class="fit-buttons">
              <button class="fit-btn" class:active={cameraFormeVal === "cadre"} onclick={() => onCameraFormeClick("cadre")}>{$t("carteEdition.formeCadreScene")}</button>
              <button class="fit-btn" class:active={cameraFormeVal === "cercle"} onclick={() => onCameraFormeClick("cercle")}>{$t("carteEdition.formeCercle")}</button>
              <button class="fit-btn" class:active={cameraFormeVal === "carre"} onclick={() => onCameraFormeClick("carre")}>{$t("carteEdition.formeCarre")}</button>
            </div>
            {#if cameraFormeVal !== "cadre"}
              <button type="button" class="toggle-row" onclick={onToggleCameraBord}>
                <span class="toggle-label">{$t("carteEdition.cameraBord")}</span>
                <span class="etat-chip" class:actif={cameraBordVal}>{cameraBordVal ? $t("commun.on") : $t("commun.off")}</span>
              </button>
            {/if}
            <!-- Cadre sur ce widget (sansCadre) + accès modale Cadres —
                 les widgets caméra suivent les mêmes règles de cadre. -->
            <div class="section-titre">{$t("carteEdition.sectionCadre")}</div>
            <button type="button" class="toggle-row" onclick={onToggleCadreWidget}>
              <span class="toggle-label">{$t("carteEdition.cadreWidget")}</span>
              <span class="etat-chip" class:actif={widgetAvecCadre}>{widgetAvecCadre ? $t("commun.on") : $t("commun.off")}</span>
            </button>
            <button class="action" onclick={onOuvrirCadres}>{$t("carteEdition.cadrePersonnaliser")}</button>
            <!-- Visibilité de la caméra en diffusion (cameraCachee) —
                 équivalent de l'œil d'OBS : masque sans supprimer le
                 widget. Même action que le badge œil au survol. -->
            <div class="section-titre">{$t("carteEdition.sectionVisibilite")}</div>
            <button type="button" class="toggle-row" onclick={onToggleCameraCachee}>
              <span class="toggle-label">{$t("carteEdition.cameraDiffusion")}</span>
              <span class="etat-chip" class:actif={!cameraCacheeVal}>{cameraCacheeVal ? $t("commun.off") : $t("commun.on")}</span>
            </button>
            <p class="hint">{$t("carteEdition.cameraDiffusionHint")}</p>
            <!-- Fond virtuel « fond vert virtuel » (plugin OBS local) —
                 voir la plaque noire disparaître derrière la personne.
                 Plugin absent → installation automatique au toggle. -->
            <div class="section-titre">{$t("carteEdition.sectionFondVirtuel")}</div>
            <button type="button" class="toggle-row" onclick={onToggleCameraFondVert} disabled={fondVertEnCours}>
              <span class="toggle-label">{$t("carteEdition.fondVert")}</span>
              <span class="etat-chip" class:actif={cameraFondVertVal}>
                {fondVertEnCours ? $t("carteEdition.fondVertInstallation") : cameraFondVertVal ? $t("commun.on") : $t("commun.off")}
              </span>
            </button>
            <p class="hint">{$t("carteEdition.fondVertHint")}</p>
            {#if fondVertInfo}
              <p class="hint" style="color: var(--message-ok-color);">✓ {fondVertInfo}</p>
            {/if}
            {#if fondVertErreur}
              <p class="hint" style="color: var(--message-error-color);">⚠ {fondVertErreur}</p>
            {/if}
          {:else}
            <!-- Barre d'onglets segmentée : piste encastrée à cellules
                 égales (1 rangée ; grille 2x2 quand l'onglet Lecture vidéo
                 s'ajoute). Recette VISUELLEMENT DISTINCTE des .fit-btn —
                 la navigation doit se lire comme une navigation. -->
            <div class="tabs" class:quatre={selectedKind === "video"}>
              <button class="tab" class:active={tabWidget === "contenu"} onclick={() => tabWidget = "contenu"} title={$t("carteEdition.tabContenu")}>{$t("carteEdition.tabContenu")}</button>
              <button class="tab" class:active={tabWidget === "transformation"} onclick={() => tabWidget = "transformation"} title={$t("carteEdition.tabTransformation")}>{$t("carteEdition.tabTransformation")}</button>
              <button class="tab" class:active={tabWidget === "effets"} onclick={() => tabWidget = "effets"} title={$t("carteEdition.tabEffets")}>{$t("carteEdition.tabEffets")}</button>
              {#if selectedKind === "video"}
                <button class="tab" class:active={tabWidget === "lecture"} onclick={() => tabWidget = "lecture"} title={$t("commun.lecture")}>{$t("commun.lecture")}</button>
              {/if}
            </div>

            {#if tabWidget === "contenu"}
              {#if !isCameraWidget}
                <div class="section-titre">{$t("carteEdition.sectionMedia")}</div>
                <button class="action primary" onclick={importMedia}>{selectedWidget?.media ? $t("carteEdition.remplacerMedia") : $t("carteEdition.importerMedia")}</button>
                {#if selectedWidget?.media}
                  <button class="action danger" onclick={onClearMedia}>{$t("carteEdition.supprimerMedia")}</button>
                {/if}
              {/if}
              {#if isChatWidget}
                <div class="section-titre">{$t("carteEdition.filtreChat")}</div>
                <div class="fit-buttons">
                  {#each FILTRES as f}
                    <button class="fit-btn" class:active={chatFiltreVal === f} onclick={() => onFiltreClick(f)}>{nomsFiltres[f]}</button>
                  {/each}
                </div>
                <div class="section-titre">{$t("carteEdition.miseEnForme")}</div>
                <div class="fit-buttons">
                  {#each CHAT_STYLES as s}
                    <button class="fit-btn" class:active={chatStyleVal === s} onclick={() => onChatStyleClick(s)}>{nomsChatStyles[s]}</button>
                  {/each}
                </div>
                {@render sliderLigne($t("carteEdition.taillePolice"), chatTaille, 8, 48, 1, onTailleInput, onTailleChange, (v) => v + "px")}
                <button class="action" onclick={onDetacher}>
                  {popoutOpen ? $t("carteEdition.reattacher") : $t("carteEdition.detacher")}
                </button>
              {:else if isInputViewerWidget}
                <div class="section-titre">{$t("carteEdition.mode")}</div>
                <div class="fit-buttons">
                  <button class="fit-btn" class:active={inputModeVal === "clavier"} onclick={() => selectedId && setInputViewerConfig(selectedId, { inputMode: "clavier" })}>{$t("carteEdition.modeClavier")}</button>
                  <button class="fit-btn" class:active={inputModeVal === "souris"} onclick={() => selectedId && setInputViewerConfig(selectedId, { inputMode: "souris" })}>{$t("carteEdition.modeSouris")}</button>
                  <button class="fit-btn" class:active={inputModeVal === "numpad"} onclick={() => selectedId && setInputViewerConfig(selectedId, { inputMode: "numpad" })}>Numpad</button>
                  <button class="fit-btn" class:active={inputModeVal === "manette"} onclick={() => selectedId && setInputViewerConfig(selectedId, { inputMode: "manette" })}>{$t("carteEdition.modeManette")}</button>
                </div>
                {#if inputModeVal === "clavier"}
                  <div class="section-titre">{$t("carteEdition.layout")}</div>
                  <div class="fit-buttons">
                    <button class="fit-btn" class:active={inputLayoutVal === "azerty"} onclick={() => selectedId && setInputViewerConfig(selectedId, { inputLayout: "azerty" })}>AZERTY</button>
                    <button class="fit-btn" class:active={inputLayoutVal === "qwerty"} onclick={() => selectedId && setInputViewerConfig(selectedId, { inputLayout: "qwerty" })}>QWERTY</button>
                  </div>
                {/if}
                {#if inputModeVal === "manette"}
                  <div class="section-titre">{$t("carteEdition.skin")}</div>
                  <div class="fit-buttons">
                    <button class="fit-btn" class:active={inputSkinVal === "xbox"} onclick={() => selectedId && setInputViewerConfig(selectedId, { inputSkin: "xbox" })}>Xbox</button>
                    <button class="fit-btn" class:active={inputSkinVal === "ps"} onclick={() => selectedId && setInputViewerConfig(selectedId, { inputSkin: "ps" })}>PlayStation</button>
                    <button class="fit-btn" class:active={inputSkinVal === "8bitdo"} onclick={() => selectedId && setInputViewerConfig(selectedId, { inputSkin: "8bitdo" })}>8BitDo</button>
                  </div>
                  <button class="action" onclick={() => (bindingModalOpen = true)}>{$t("carteEdition.binderTouches")}</button>
                {/if}
                <div class="field">
                  <span class="field-label">{$t("carteEdition.couleurTouches")}</span>
                  <input type="color" value={inputCouleurVal} oninput={(e) => selectedId && setInputViewerConfigLocal(selectedId, { inputCouleur: (e.target as HTMLInputElement).value })} onchange={onInputCouleurChange} />
                </div>
                <button type="button" class="toggle-row" onclick={toggleCaptureInput}>
                  <span class="toggle-label">{$t("carteEdition.captureEntrees")}</span>
                  <span class="etat-chip" class:actif={captureActive}>{captureActive ? $t("commun.on") : $t("commun.off")}</span>
                </button>
                <p class="hint">
                  {captureActive ? $t("carteEdition.captureActiveHint") : $t("carteEdition.captureInactiveHint")}
                </p>
              {:else if isSpeedrunWidget}
                {#if $erreurSpeedrun}
                  <p class="hint" style="color: var(--message-error-color);">⚠ {$erreurSpeedrun}</p>
                {/if}
                <div class="field">
                  <span class="field-label">{$t("carteEdition.jeu")}</span>
                  <span class="media-val">
                    {#if $nomJeuLss || $nomCategorieLss}
                      {$nomJeuLss}{#if $nomCategorieLss} — {$nomCategorieLss}{/if}
                    {:else}
                      <span class="hint">{$t("widgetSpeedrun.aucunSplits")}</span>
                    {/if}
                  </span>
                </div>
                {#if $estConnecte}
                  <p class="hint" style="color: var(--message-ok-color);">● {$t("carteEdition.connecte")}{$nomProcessus ? ` (${$nomProcessus})` : ""}</p>
                {:else if $estActif}
                  <p class="hint" style="color: var(--message-user-action-color);">○ {$t("widgetSpeedrun.enAttente")}</p>
                {/if}
                <button class="action" onclick={onSpeedrunChoisirAsl}>
                  {#if $cheminAsl}{$t("carteEdition.aslCharge")}{:else}{$t("carteEdition.chargerAsl")}{/if}
                </button>
                <button class="action" onclick={onSpeedrunChoisirLss}>
                  {#if $cheminLss}{$t("carteEdition.lssCharge")}{:else}{$t("carteEdition.chargerLss")}{/if}
                </button>
                <div class="speedrun-ctrl-row">
                  {#if !$estActif}
                    <button class="action speedrun-start" onclick={onSpeedrunDemarrer}>▶ {$t("widgetSpeedrun.start")}</button>
                  {:else}
                    <button class="action speedrun-stop" onclick={onSpeedrunArreter}>■ {$t("widgetSpeedrun.stop")}</button>
                  {/if}
                  <button class="action" onclick={() => actionManuelle("split")} disabled={!$estActif}>{$t("widgetSpeedrun.split")}</button>
                  <button class="action" onclick={() => actionManuelle("skip")} disabled={!$estActif}>{$t("widgetSpeedrun.skip")}</button>
                  <button class="action" onclick={() => actionManuelle("undo")} disabled={!$estActif}>{$t("widgetSpeedrun.undo")}</button>
                  <button class="action" onclick={() => actionManuelle("reset")} disabled={!$estActif}>{$t("widgetSpeedrun.reset")}</button>
                  <button class="action" onclick={() => actionManuelle("pause")} disabled={!$estActif}>
                    {#if $phaseTimer === "Paused"}{$t("widgetSpeedrun.resume")}{:else}{$t("commun.pause")}{/if}
                  </button>
                </div>
                <div class="speedrun-toggles">
                  <label class="setting-toggle" class:active={srStartActif}>
                    <input type="checkbox" checked={srStartActif} onchange={onSpeedrunToggleStart} />
                    <span>{$t("widgetSpeedrun.autoStart")}</span>
                  </label>
                  <label class="setting-toggle" class:active={srSplitActif}>
                    <input type="checkbox" checked={srSplitActif} onchange={onSpeedrunToggleSplit} />
                    <span>{$t("widgetSpeedrun.autoSplit")}</span>
                  </label>
                  <label class="setting-toggle" class:active={srResetActif}>
                    <input type="checkbox" checked={srResetActif} onchange={onSpeedrunToggleReset} />
                    <span>{$t("widgetSpeedrun.autoReset")}</span>
                  </label>
                  <label class="setting-toggle" class:active={srMetroActifVal}>
                    <input type="checkbox" checked={srMetroActifVal} onchange={onSrMetroToggle} />
                    <span>{$t("widgetSpeedrun.metronome")}</span>
                  </label>
                </div>
                {#if srMetroActifVal}
                  <!-- Battement BPM pendant la run — entendu sur le DASHBOARD
                       uniquement, jamais dans le stream (hint ci-dessous).
                       Le bouton Tester joue 4 tics sans attendre une run ;
                       désactivé pendant une run (le vrai battement joue). -->
                  {@render sliderLigne($t("widgetSpeedrun.metronomeBpm"), srMetroBpmVal, 1, 400, 1, onSrMetroBpmInput, onSrMetroBpmChange, (v) => Math.round(v) + " BPM")}
                  {@render sliderLigne($t("widgetSpeedrun.metronomeVolume"), srMetroVolPct, 0, 100, 1, onSrMetroVolInput, onSrMetroVolChange, (v) => v + "%")}
                  <button class="action" onclick={onSrMetroTest} disabled={$battementEnCours}>
                    {$t("widgetSpeedrun.metronomeTest")}
                  </button>
                  <p class="hint">{$t("widgetSpeedrun.metronomeHint")}</p>
                {/if}
                <!-- Pop-out splitter : fenêtre toujours-au-dessus avec le
                     rendu du splitter ET les contrôles de run (pattern
                     raccourcis : la fenêtre émet, le dashboard exécute).
                     Le widget de la scène et la diffusion restent inchangés. -->
                <button class="action" onclick={onSrDetacher}>
                  {srPopoutOpen ? $t("carteEdition.reattacher") : $t("carteEdition.detacher")}
                </button>
                <!-- Export HTML autonome de la tentative (tableau des
                     splits) — dialog « Enregistrer sous » côté Rust. -->
                <button class="action" onclick={() => exporterSplits().catch(() => {})}>
                  {$t("widgetSpeedrun.exporter")}
                </button>
                <div class="field">
                  <span class="field-label">{$t("commun.police")}</span>
                  <select class="police-select" value={srPoliceVal} onchange={onSrPoliceChange}>
                    {#each POLICES_TITRE as p (p.id)}
                      <option value={p.id} style="font-family: '{p.id}', sans-serif;">{p.nom}</option>
                    {/each}
                  </select>
                </div>
                <div class="field">
                  <span class="field-label">{$t("widgetSpeedrun.disposition")}</span>
                  <select class="police-select" value={srLayoutVal} onchange={onSrLayoutChange}>
                    <option value="auto">{$t("widgetSpeedrun.dispoAuto")}</option>
                    <option value="colonne">{$t("widgetSpeedrun.dispoColonne")}</option>
                    <option value="ligne">{$t("widgetSpeedrun.dispoLigne")}</option>
                  </select>
                </div>
                {@render sliderLigne($t("carteEdition.taillePolice"), srTailleVal, 8, 48, 1, onSrTailleInput, onSrTailleChange, (v) => v + "px")}
                <!-- Layout Editor façon LiveSplit : pilote UNIQUEMENT les
                     éléments internes du splitter (en-tête/timer/méta/
                     segments) via les champs srXxx du widget. -->
                <button class="action" onclick={() => speedrunLayoutModalOpen.set(true)}>{$t("speedrunLayout.ouvrir")}</button>
              {:else if isGoalWidget}
                <div class="section-titre">{$t("carteEdition.plateforme")}</div>
                <div class="fit-buttons">
                  {#each GOAL_PLATEFORMES as p (p.id)}
                    <button class="fit-btn" class:active={goalPlat === p.id} onclick={() => onGoalPlateforme(p.id)}>{p.label}</button>
                  {/each}
                </div>
                {#if goalPlat === "twitch"}
                  <!-- Creator Goals déclarés dans le Creator Dashboard Twitch :
                       liste auto-fetchée à l'ouverture, clic = lier / re-clic =
                       délier, valeurs live (frame goal-twitch → goalsTwitch). -->
                  <div class="section-titre">{$t("carteEdition.goalsTwitch")}</div>
                  {#if !twitchConnecte}
                    <p class="hint">{$t("carteEdition.goalsNonConnecte")}</p>
                  {:else if goalsChargement && goalsTwitchListe === null}
                    <p class="hint">{$t("carteEdition.goalsChargement")}</p>
                  {:else if goalsErreur === "need_reauth"}
                    <p class="hint">{$t("carteEdition.goalsReauth")}</p>
                    <button class="action" onclick={() => void connecterTwitch()}>{$t("carteEdition.reconnecterTwitch")}</button>
                  {:else if goalsErreur}
                    <p class="hint">{goalsErreur}</p>
                    <button class="action" onclick={() => void chargerGoalsTwitch()}>{$t("carteEdition.goalsRecharger")}</button>
                  {:else if goalsTwitchListe !== null && goalsTwitchListe.length === 0}
                    <p class="hint">{$t("carteEdition.goalsVide")}</p>
                  {:else if goalsTwitchListe !== null}
                    <div class="goal-liste">
                      {#each goalsTwitchListe as g (g.id)}
                        {@const gl = $goalsTwitch[g.id] ?? g}
                        <button class="goal-item" class:lie={goalLie === g.id} onclick={() => onGoalLier(gl)}>
                          <span class="goal-item-haut">
                            <span class="goal-item-nom">{gl.description || $t("goal.ttype." + gl.goal_type)}</span>
                            {#if goalLie === g.id}<span class="goal-item-badge">{$t("carteEdition.goalLie")}</span>{/if}
                            {#if gl.termine}<span class="goal-item-badge">{$t("carteEdition.goalTermine")}</span>{/if}
                          </span>
                          <span class="goal-item-chiffres">{gl.actuel.toLocaleString($localeTag)} / {gl.cible.toLocaleString($localeTag)}</span>
                          <span class="goal-item-bar"><span class="goal-item-fill" style:width={(gl.cible > 0 ? Math.min(100, Math.round((gl.actuel / gl.cible) * 100)) : 0) + "%"}></span></span>
                        </button>
                      {/each}
                    </div>
                  {/if}
                {/if}
                {#if goalLie}
                  <!-- Mode lié : actuel/cible viennent de Twitch — les
                       contrôles manuels sont masqués. -->
                  {#if goalsTwitchListe !== null && !goalsTwitchListe.some((g) => g.id === goalLie)}
                    <p class="hint">{$t("carteEdition.goalDisparu")}</p>
                  {/if}
                  <button class="action" onclick={() => selectedId && setGoalConfig(selectedId, { goalId: undefined })}>{$t("carteEdition.goalDelier")}</button>
                  <p class="hint">{$t("carteEdition.goalLieHint")}</p>
                {:else}
                  <div class="section-titre">{$t("carteEdition.metrique")}</div>
                  <div class="fit-buttons">
                    {#each GOAL_METRIQUES[goalPlat] as m (m)}
                      <button class="fit-btn" class:active={goalMet === m} onclick={() => selectedId && setGoalConfig(selectedId, { goalMetrique: m })}>{$t("goal.metrique." + m)}</button>
                    {/each}
                  </div>
                  <div class="field">
                    <span class="field-label">{$t("carteEdition.cible")}</span>
                    <input type="number" min="1" step="1" value={goalCibleVal} onchange={onGoalCibleChange} />
                  </div>
                  <p class="hint">
                    {$t("carteEdition.actuel", { v: goalActuel === undefined ? $t("carteEdition.actuelNA") : goalActuel.toLocaleString($localeTag) })}
                  </p>
                  <button class="action" disabled={goalSuggere === undefined} onclick={() => selectedId && goalSuggere !== undefined && setGoalConfig(selectedId, { goalCible: goalSuggere })}>
                    {$t("carteEdition.suggerer", { v: goalSuggere === undefined ? "N/A" : goalSuggere.toLocaleString($localeTag) })}
                  </button>
                  <p class="hint">{$t("carteEdition.goalHint")}</p>
                {/if}
              {:else if isMusiqueWidget}
                <!-- Widget musique : mini-lecteur Winamp rendu dans la
                     diffusion (LCD marquee + équalizer + barre). Deux modes :
                     player = tout, eq = bandes de spectre seules. -->
                <div class="section-titre">{$t("carteEdition.musiqueMode")}</div>
                <div class="fit-buttons">
                  <button class="fit-btn" class:active={musiqueModeVal === "player"} onclick={() => selectedId && setMusiqueConfig(selectedId, "player")}>{$t("carteEdition.musiqueModePlayer")}</button>
                  <button class="fit-btn" class:active={musiqueModeVal === "eq"} onclick={() => selectedId && setMusiqueConfig(selectedId, "eq")}>{$t("carteEdition.musiqueModeEq")}</button>
                </div>
                <p class="hint">{$t("carteEdition.musiqueHint")}</p>
              {:else}
                <!-- Widget média : fenêtre de capture + source OBS -->
                <div class="section-titre">{natifMode ? $t("carteEdition.sectionFenetreNatif") : $t("carteEdition.sectionFenetreObs")}</div>
                <button type="button" class="toggle-row" onclick={onToggleTrou}>
                  <span class="toggle-label">{$t("carteEdition.fenetreCapture")}</span>
                  <span class="etat-chip" class:actif={selectedTrou}>{selectedTrou ? $t("commun.on") : $t("commun.off")}</span>
                </button>
                {#if selectedTrou}
                  {#if selectedObsSource}
                    <button class="action danger" onclick={onDeleteObsSource}>{$t("carteEdition.sourceObsSupprimer")}</button>
                  {:else}
                    <button class="action" onclick={onOpenObsDialog}>{natifMode ? $t("carteEdition.sourceLierNatif") : $t("carteEdition.sourceObsLier")}</button>
                  {/if}
                {/if}
                {#if isMediaWidget}
                  <!-- Carrousel d'images (mediaSlides) : liste non vide =
                       prioritaire sur le média unique du widget. -->
                  {@render carrouselBloc(widgetSlides, widgetSlidesSecVal, onAjouterSlidesMedia, retirerSlideMedia, deplacerSlideMedia, onWidgetSlidesSecInput, onWidgetSlidesSecChange, viderSlidesMedia)}
                {/if}
              {/if}
              <!-- Cadre sur ce widget (sansCadre — opt-out par instance,
                   tous types) + accès modale Cadres. Généralise l'ex-toggle
                   « widget ↔ flottant » du DropBar. -->
              <div class="section-titre">{$t("carteEdition.sectionCadre")}</div>
              <button type="button" class="toggle-row" onclick={onToggleCadreWidget}>
                <span class="toggle-label">{$t("carteEdition.cadreWidget")}</span>
                <span class="etat-chip" class:actif={widgetAvecCadre}>{widgetAvecCadre ? $t("commun.on") : $t("commun.off")}</span>
              </button>
              <button class="action" onclick={onOuvrirCadres}>{$t("carteEdition.cadrePersonnaliser")}</button>
              {#if !cadreSceneActif}
                <p class="hint">{$t("carteEdition.cadreSceneInactive")}</p>
              {/if}
            {:else if tabWidget === "transformation"}
              <Gizmo3D />
              {@render controlesTransformation()}
            {:else if tabWidget === "effets"}
              {@render controlesEffetsWidget()}
            {:else if tabWidget === "lecture"}
              {@render controlesLecture()}
            {/if}
            <!-- Pied de carte : Dupliquer (action immédiate, bascule sur
                 l'onglet Transformation pour orienter la copie via le gizmo
                 + les boutons de retournement) puis zone danger isolée.
                 Supprimer reste l'unique bouton rouge de la carte. -->
            <div class="carte-pied">
              <button class="action" onclick={onDupliquer}>{$t("commun.dupliquer")}</button>
            </div>
            <div class="carte-danger">
              <button class="action danger" onclick={onSupprimerClick}>{$t("carteEdition.supprimerWidget")}</button>
            </div>
          {/if}
        {:else}
          <!-- ====== ÉDITION FOND ====== -->
          {#if bgObsSource}
            <!-- Fond = capture app/jeu (prioritaire sur le média — le média
                 stocké n'est pas peint tant que la capture est active). -->
            <p class="hint">{$t("carteEdition.fondCaptureHint")}</p>
            <button class="action" onclick={() => (obsFondDialogOpen = true)}>{$t("carteEdition.changerCapture")}</button>
            <button class="action danger" onclick={() => { void retirerFondCapture(); }}>{$t("carteEdition.retirerCapture")}</button>
            <button class="action" onclick={() => titreFondModalOpen.set(true)}>
              {hasBgTitre ? $t("carteEdition.titreFondModifier") : $t("carteEdition.titreFondAjouter")}
            </button>
          {:else if !hasFond}
            <button class="action primary" onclick={importFond}>{$t("carteEdition.importerFond")}</button>
            <button class="action" onclick={onAjouterSlidesFond}>{$t("carteEdition.carrouselCreer")}</button>
            <button class="action" onclick={() => (obsFondDialogOpen = true)}>{$t("carteEdition.appEnFond")}</button>
            <p class="hint">{$t("carteEdition.aucunFond")}</p>
          {:else}
            <div class="tabs" class:quatre={bgKind === "video"}>
              <button class="tab" class:active={tabFond === "contenu"} onclick={() => tabFond = "contenu"} title={$t("carteEdition.tabContenu")}>{$t("carteEdition.tabContenu")}</button>
              <button class="tab" class:active={tabFond === "transformation"} onclick={() => tabFond = "transformation"} title={$t("carteEdition.tabTransformation")}>{$t("carteEdition.tabTransformation")}</button>
              <button class="tab" class:active={tabFond === "effets"} onclick={() => tabFond = "effets"} title={$t("carteEdition.tabEffets")}>{$t("carteEdition.tabEffets")}</button>
              {#if bgKind === "video"}
                <button class="tab" class:active={tabFond === "lecture"} onclick={() => tabFond = "lecture"} title={$t("commun.lecture")}>{$t("commun.lecture")}</button>
              {/if}
            </div>

            {#if tabFond === "contenu"}
              <div class="section-titre">{$t("carteEdition.sectionMedia")}</div>
              <button class="action primary" onclick={importFond}>{$t("carteEdition.remplacerFond")}</button>
              <button class="action danger" onclick={() => clearFond()}>{$t("carteEdition.supprimerFond")}</button>
              <button class="action" onclick={() => (obsFondDialogOpen = true)}>{$t("carteEdition.appEnFond")}</button>
              <button class="action" onclick={() => titreFondModalOpen.set(true)}>
                {hasBgTitre ? $t("carteEdition.titreFondModifier") : $t("carteEdition.titreFondAjouter")}
              </button>
              <!-- Carrousel d'images du fond (bgSlides) : liste non vide =
                   prioritaire sur bgMedia (bgObsSource reste au-dessus). -->
              {@render carrouselBloc(bgSlides, bgSlidesSecVal, onAjouterSlidesFond, retirerSlideFond, deplacerSlideFond, onBgSlidesSecInput, onBgSlidesSecChange, viderSlidesFond)}
            {:else if tabFond === "transformation"}
              <Gizmo3D cible="fond" />
              <div class="section-titre-row">
                <span class="section-titre">{$t("carteEdition.affichageFond")}</span>
                <button class="compacte-lien" onclick={onBgReset}>{$t("carteEdition.resetTransformation")}</button>
              </div>
              <div class="fit-buttons">
                {#each FIT_MODES as mode}
                  <button class="fit-btn" class:active={bgFit === mode} onclick={() => onBgFitClick(mode)}>{labelFit(mode)}</button>
                {/each}
              </div>
              {@render sliderLigne($t("carteEdition.zoom"), bgZoom, 0.2, 5, 0.1, onBgZoomInput, onBgZoomChange, (v) => v.toFixed(2))}
              {@render sliderLigne($t("carteEdition.rotation"), bgRot, -180, 180, 5, onBgRotInput, onBgRotChange, (v) => Math.round(v) + "°")}
              {@render sliderLigne($t("carteEdition.offsetX"), bgOffsetX, -2000, 2000, 1, onBgOffsetXInput, onBgOffsetChange, (v) => Math.round(v) + "px")}
              {@render sliderLigne($t("carteEdition.offsetY"), bgOffsetY, -2000, 2000, 1, onBgOffsetYInput, onBgOffsetChange, (v) => Math.round(v) + "px")}
            {:else if tabFond === "effets"}
              <div class="lien-row">
                <button class="compacte-lien" onclick={onBgResetEffets}>{$t("carteEdition.resetEffets")}</button>
              </div>
              {@render sliderLigne($t("carteEdition.luminosite"), bgLum, -100, 100, 1, onBgLumInput, onBgEffetChange, (v) => String(Math.round(v)))}
              {@render sliderLigne($t("carteEdition.contraste"), bgContraste, -100, 100, 1, onBgContrasteInput, onBgEffetChange, (v) => String(Math.round(v)))}
              {@render sliderLigne($t("carteEdition.teinte"), bgTeinte, 0, 360, 1, onBgTeinteInput, onBgEffetChange, (v) => Math.round(v) + "°")}
              {@render sliderLigne($t("carteEdition.flou"), bgFlou, 0, 20, 0.5, onBgFlouInput, onBgEffetChange, (v) => v.toFixed(1) + "px")}
              {@render sliderLigne($t("carteEdition.pixelisation"), bgPixel, 0, 50, 1, onBgPixelInput, onBgEffetChange, (v) => String(Math.round(v)))}
              <div class="section-titre">{$t("carteEdition.miroirFond")}</div>
              <div class="fit-buttons">
                <button class="fit-btn" class:active={bgMiroirH} onclick={() => onBgMiroir("h")}>{$t("carteEdition.miroirH")}</button>
                <button class="fit-btn" class:active={bgMiroirV} onclick={() => onBgMiroir("v")}>{$t("carteEdition.miroirV")}</button>
              </div>
              {#if bgKind === "image"}
                <div class="section-titre">Morphing</div>
                <div class="fit-buttons">
                  <button class="fit-btn" class:active={$morphMode} onclick={() => morphMode.update((m) => !m)}>Morphing</button>
                </div>
                {#if $morphMode}
                  <div class="morph-bloc">
                    <div class="fit-buttons">
                      <button class="fit-btn" class:active={$morphSens === "agrandir"} onclick={() => morphSens.set("agrandir")}>{$t("carteEdition.agrandir")}</button>
                      <button class="fit-btn" class:active={$morphSens === "retrecir"} onclick={() => morphSens.set("retrecir")}>{$t("carteEdition.retrecir")}</button>
                    </div>
                    {@render sliderLigne($t("carteEdition.intensite"), $morphIntensite, 10, 200, 5, (e) => morphIntensite.set(+(e.target as HTMLInputElement).value), () => {}, (v) => v + "%")}
                    {@render sliderLigne($t("carteEdition.rayon"), Math.round($morphRayon * 100), 5, 50, 1, (e) => morphRayon.set(+(e.target as HTMLInputElement).value / 100), () => {}, (v) => v + "%")}
                    <button class="compacte-lien" onclick={onResetMorphsFond}>{$t("carteEdition.resetMorphing")}</button>
                    <p class="hint">{$t("carteEdition.morphHintFond")}</p>
                  </div>
                {/if}
              {/if}
            {:else if tabFond === "lecture"}
              {#if bgKind === "video"}
                <PlayerBar videoEl={fondVideoEl} mediaPaused={bgPaused} mediaTime={bgTime} loopA={bgLoopA} loopB={bgLoopB} onTogglePlay={onBgTogglePlay} onSeek={onBgSeek} onSeekLive={onBgSeekLive} onLoopChange={onBgLoopChange} />
              {/if}
            {/if}
          {/if}
          <!-- Cadre sur le bord du canvas (bordActif — portée de la
               CadreConfig unifiée) + accès modale Cadres. -->
          <div class="section-titre">{$t("carteEdition.sectionCadre")}</div>
          {#if cadreSceneActif}
            <button type="button" class="toggle-row" onclick={onToggleCadreBord}>
              <span class="toggle-label">{$t("carteEdition.cadreBord")}</span>
              <span class="etat-chip" class:actif={cadreBordActif}>{cadreBordActif ? $t("commun.on") : $t("commun.off")}</span>
            </button>
          {/if}
          <button class="action" onclick={onOuvrirCadres}>{$t("carteEdition.cadrePersonnaliser")}</button>
          {#if !cadreSceneActif}
            <p class="hint">{$t("carteEdition.cadreSceneInactive")}</p>
          {/if}
        {/if}
      </div>
    {/if}
  </div>
{/if}

{#if obsDialogOpen && selectedId}
  <ObsSourceDialog widgetId={selectedId} onClose={onCloseObsDialog} />
{/if}

<!-- Fond de scène = capture app/jeu (ObsSourceDialog mode "fond") -->
{#if obsFondDialogOpen}
  <ObsSourceDialog mode="fond" onClose={() => (obsFondDialogOpen = false)} />
{/if}

{#if bindingModalOpen && selectedId}
  <InputBindingModal
    widgetId={selectedId}
    mapping={selectedWidget?.inputMapping ?? { buttons: [0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15], dpadInvertY: false, dpadInvertX: false }}
    onFermer={() => (bindingModalOpen = false)}
  />
{/if}

<!-- ===== Snippets partagés ===== -->
{#snippet sliderLigne(label: string, valeur: number, min: number, max: number, step: number, oninput: (e: Event) => void, onchange: () => void, fmt?: (v: number) => string)}
  <div class="slider-ligne">
    <span class="slider-label">{label}</span>
    <input class="range" type="range" {min} {max} {step} value={valeur} {oninput} {onchange} />
    <span class="slider-val">{fmt ? fmt(valeur) : String(valeur)}</span>
  </div>
{/snippet}

<!-- Bloc partagé du carrousel d'images (widget média + fond de scène) :
     liste ordonnée de slides (défilement séquentiel + fondu enchaîné
     côté diffusion), ajout/retrait/réordonnancement + intervalle. -->
{#snippet carrouselBloc(
  slides: MediaSlide[],
  secVal: number,
  onAjouter: () => void,
  onRetirer: (i: number) => void,
  onDeplacer: (i: number, dir: -1 | 1) => void,
  onSecInput: (e: Event) => void,
  onSecChange: () => void,
  onVider: () => void
)}
  <div class="section-titre">{$t("carteEdition.carrousel")}</div>
  {#if slides.length}
    <div class="slides-list">
      {#each slides as slide, i}
        <div class="slide-ligne">
          <span class="slide-nom" title={slide.rel}>{i + 1}. {slide.nom}</span>
          <button class="slide-btn" title={$t("carteEdition.carrouselMonter")} onclick={() => onDeplacer(i, -1)} disabled={i === 0}>↑</button>
          <button class="slide-btn" title={$t("carteEdition.carrouselDescendre")} onclick={() => onDeplacer(i, 1)} disabled={i === slides.length - 1}>↓</button>
          <button class="slide-btn slide-del" title={$t("carteEdition.carrouselRetirer")} onclick={() => onRetirer(i)}>✕</button>
        </div>
      {/each}
    </div>
    {@render sliderLigne($t("carteEdition.carrouselSec"), secVal, 1, 60, 1, onSecInput, onSecChange, (v) => Math.round(v) + " s")}
  {/if}
  <button class="action" onclick={onAjouter}>{slides.length ? $t("carteEdition.carrouselAjouter") : $t("carteEdition.carrouselCreer")}</button>
  {#if slides.length}
    <button class="action danger" onclick={onVider}>{$t("carteEdition.carrouselVider")}</button>
  {/if}
{/snippet}

{#snippet controlesTransformation()}
  <div class="section-titre-row">
    <span class="section-titre">{$t("carteEdition.affichage")}</span>
    <button class="compacte-lien" onclick={onResetMediaTransformation}>{$t("carteEdition.resetTransformation")}</button>
  </div>
  <div class="fit-buttons">
    {#each FIT_MODES as mode}
      <button class="fit-btn" class:active={currentFit === mode} onclick={() => onFitClick(mode)}>{labelFit(mode)}</button>
    {/each}
  </div>
  {@render sliderLigne($t("carteEdition.zoom"), currentZoom, 0.2, 5, 0.1, onZoomInput, onZoomChange, (v) => v.toFixed(2))}
  {@render sliderLigne($t("carteEdition.rotation"), currentRot, -180, 180, 5, onRotInput, onRotChange, (v) => Math.round(v) + "°")}
  {@render sliderLigne($t("carteEdition.offsetX"), currentOffsetX, -2000, 2000, 1, onOffsetXInput, onOffsetChange, (v) => Math.round(v) + "px")}
  {@render sliderLigne($t("carteEdition.offsetY"), currentOffsetY, -2000, 2000, 1, onOffsetYInput, onOffsetChange, (v) => Math.round(v) + "px")}
  <!-- Retournement (ex-onglet Dupliquer) : flips exacts ±180° + reset —
       le gizmo pleine amplitude au-dessus couvre les inclinaisons. -->
  <div class="section-titre">{$t("carteEdition.retournement")}</div>
  <div class="fit-buttons">
    <button class="fit-btn" onclick={() => onMiroir("h")}>{$t("carteEdition.miroirH")}</button>
    <button class="fit-btn" onclick={() => onMiroir("v")}>{$t("carteEdition.miroirV")}</button>
    <button class="fit-btn" onclick={() => onMiroir("reset")}>{$t("carteEdition.reset")}</button>
  </div>
  <p class="hint">{$t("carteEdition.gizmoHint")}</p>
{/snippet}

{#snippet controlesEffetsWidget()}
  <div class="lien-row">
    <button class="compacte-lien" onclick={onResetMediaEffets}>{$t("carteEdition.resetEffets")}</button>
  </div>
  {@render sliderLigne($t("carteEdition.luminosite"), currentLum, -100, 100, 1, onLumInput, onEffetChange, (v) => String(Math.round(v)))}
  {@render sliderLigne($t("carteEdition.contraste"), currentContraste, -100, 100, 1, onContrasteInput, onEffetChange, (v) => String(Math.round(v)))}
  {@render sliderLigne($t("carteEdition.teinte"), currentTeinte, 0, 360, 1, onTeinteInput, onEffetChange, (v) => Math.round(v) + "°")}
  {@render sliderLigne($t("carteEdition.flou"), currentFlou, 0, 20, 0.5, onFlouInput, onEffetChange, (v) => v.toFixed(1) + "px")}
  {@render sliderLigne($t("carteEdition.pixelisation"), currentPixel, 0, 50, 1, onPixelInput, onEffetChange, (v) => String(Math.round(v)))}
  {#if selectedWidget?.media}
    <div class="section-titre">{$t("carteEdition.miroirMedia")}</div>
    <div class="fit-buttons">
      <button class="fit-btn" class:active={currentMiroirH} onclick={() => onMiroirMedia("h")}>{$t("carteEdition.miroirH")}</button>
      <button class="fit-btn" class:active={currentMiroirV} onclick={() => onMiroirMedia("v")}>{$t("carteEdition.miroirV")}</button>
    </div>
  {/if}
  {#if isMediaWidget && selectedKind === "image" && selectedWidget?.media}
    <div class="section-titre">Morphing</div>
    <div class="fit-buttons">
      <button class="fit-btn" class:active={$morphMode} onclick={() => morphMode.update((m) => !m)}>Morphing</button>
    </div>
    {#if $morphMode}
      <div class="morph-bloc">
        <div class="fit-buttons">
          <button class="fit-btn" class:active={$morphSens === "agrandir"} onclick={() => morphSens.set("agrandir")}>{$t("carteEdition.agrandir")}</button>
          <button class="fit-btn" class:active={$morphSens === "retrecir"} onclick={() => morphSens.set("retrecir")}>{$t("carteEdition.retrecir")}</button>
        </div>
        {@render sliderLigne($t("carteEdition.intensite"), $morphIntensite, 10, 200, 5, (e) => morphIntensite.set(+(e.target as HTMLInputElement).value), () => {}, (v) => v + "%")}
        {@render sliderLigne($t("carteEdition.rayon"), Math.round($morphRayon * 100), 5, 50, 1, (e) => morphRayon.set(+(e.target as HTMLInputElement).value / 100), () => {}, (v) => v + "%")}
        <button class="compacte-lien" onclick={() => selectedId && resetMorphsWidget(selectedId)}>{$t("carteEdition.resetMorphing")}</button>
        <p class="hint">{$t("carteEdition.morphHintWidget")}</p>
      </div>
    {/if}
  {/if}
{/snippet}

{#snippet controlesLecture()}
  {#if selectedKind === "video"}
    <PlayerBar videoEl={widgetVideoEl} mediaPaused={widgetMediaPaused} mediaTime={widgetMediaTime} loopA={widgetLoopA} loopB={widgetLoopB} onTogglePlay={onWidgetTogglePlay} onSeek={onWidgetSeek} onSeekLive={onWidgetSeekLive} onLoopChange={onWidgetLoopChange} />
  {:else}
    <p class="hint">{$t("carteEdition.pasDeVideo")}</p>
  {/if}
{/snippet}

<style>
  /* La recette de base .carte-edition est globale (app.css — partagée avec
     la sidebar). Les boutons internes gardent le fill glace neutre commun ;
     la couleur de la carte (--accent-carte) vit dans les bordures/glows/états
     actifs uniquement (IDEA02 — repère de cible conservé). */

  /* Cible widget = bordeaux foncé (pointillés + glow, palette --dash-*) */
  .cible-widget {
    --accent-carte: var(--dash-danger);
    --accent-carte-glow: color-mix(in srgb, var(--dash-danger) 45%, transparent);
  }

  /* Cible fond = orange foncé (pointillés + glow, palette --dash-*) */
  .cible-fond {
    --accent-carte: var(--dash-orange);
    --accent-carte-glow: color-mix(in srgb, var(--dash-orange) 40%, transparent);
  }

  /* Cible input-viewer = jaune foncé (pointillés + glow, palette --dash-*) */
  .cible-input-viewer {
    --accent-carte: var(--dash-jaune);
    --accent-carte-glow: color-mix(in srgb, var(--dash-jaune) 40%, transparent);
  }

  .carte-entete-row {
    display: flex;
    align-items: stretch;
    padding: 0.3rem 0.3rem 0.3rem 0.5rem;
    gap: 0.25rem;
    border-bottom: 1px solid var(--accent-carte);
  }

  .carte-entete {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.45rem 0.6rem;
    background: linear-gradient(135deg, color-mix(in srgb, var(--accent-carte) 15%, transparent), transparent);
    border: none;
    color: var(--accent-carte);
    font: inherit;
    font-size: 0.85rem;
    font-weight: 600;
    cursor: pointer;
    text-align: left;
    flex: 1;
    min-width: 0;
  }
  .carte-entete:hover {
    filter: brightness(1.15);
  }

  .carte-fleche {
    font-size: 0.75rem;
    width: 0.85rem;
    text-align: center;
    flex-shrink: 0;
  }

  .carte-titre {
    flex: 1;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    font-size: 0.8rem;
  }

  .carte-fermer {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 1.125rem;
    height: 1.125rem;
    flex-shrink: 0;
    align-self: center;
    border: 1px solid color-mix(in srgb, var(--accent-carte) 50%, transparent);
    border-radius: var(--rayon-petit);
    background: var(--btn-surface);
    box-shadow: var(--btn-inset);
    color: var(--accent-carte);
    cursor: pointer;
    font-size: 0.55rem;
    line-height: 1;
    transition: background 0.15s ease, box-shadow 0.15s ease, color 0.15s ease;
  }
  .carte-fermer:hover {
    background: var(--accent-carte);
    color: #fff;
    border-color: var(--accent-carte);
    box-shadow: var(--btn-inset-hover);
  }

  .carte-corps {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
    padding: 0.5rem 0.6rem;
    /* 45vh (ex-60vh) : libère davantage la Toolbar en dessous — la carte
       reste au-dessus de l'accordéon, mais sans l'écraser. */
    max-height: 45vh;
    min-width: 0;
    overflow-y: auto;
    scrollbar-width: thin;
    scrollbar-color: var(--accent-carte-glow) transparent;
  }

  /* ===== Sections titrées =====
     Anciennes .field-label pâles → titres de section lisibles (opacité
     pleine, majuscules espacées) : la hiérarchie n'est plus inversée, les
     catégories se voient AVANT les boutons qu'elles décrivent. La variante
     -row ajoute un slot droite pour les actions compactes (Reset…). */
  .section-titre-row {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 0.5rem;
    margin-top: 0.35rem;
  }
  .section-titre {
    font-size: 0.68rem;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    opacity: 0.85;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  /* Rangée d'actions compactes alignée à droite (reset d'effets…). */
  .lien-row {
    display: flex;
    justify-content: flex-end;
  }
  /* Action compacte texte (Reset transformation / Reset effets…) — remplace
     les anciens boutons pleine largeur « Reset … ». */
  .compacte-lien {
    background: none;
    border: none;
    color: var(--texte);
    opacity: 0.55;
    font: inherit;
    font-size: 0.68rem;
    padding: 0;
    cursor: pointer;
    white-space: nowrap;
    flex-shrink: 0;
    transition: opacity 0.15s ease;
  }
  .compacte-lien:hover {
    opacity: 0.95;
    text-decoration: underline;
  }
  .compacte-lien:disabled {
    opacity: 0.3;
    cursor: default;
    text-decoration: none;
  }

  /* ===== Toggles à libellé fixe + pastille d'état =====
     L'ancien pattern « le libellé du bouton se réécrit selon l'état »
     (« Activer… » / « Désactiver… ») laissait croire à deux boutons
     différents ; ici le libellé est FIXE et l'état vit dans la pastille
     ON/OFF à droite. */
  .toggle-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
    width: 100%;
    background: var(--btn-surface);
    color: var(--texte);
    border: 1px solid var(--bordure);
    box-shadow: var(--btn-inset);
    padding: 0.35rem 0.6rem;
    font: inherit;
    cursor: pointer;
    text-align: left;
    flex-shrink: 0;
    transition: background 0.15s ease, border-color 0.15s ease, box-shadow 0.15s ease;
  }
  .toggle-row:hover {
    background: var(--btn-surface-hover);
    border-color: var(--accent-carte);
    box-shadow: var(--btn-inset-hover);
  }
  .toggle-label {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .etat-chip {
    flex-shrink: 0;
    font-size: 0.62rem;
    font-weight: 700;
    letter-spacing: 0.04em;
    padding: 0.08rem 0.42rem;
    border-radius: 999px;
    border: 1px solid var(--bordure);
    opacity: 0.55;
    color: var(--texte);
  }
  .etat-chip.actif {
    border-color: var(--accent-carte);
    color: var(--accent-carte);
    opacity: 1;
  }

  /* ===== Pied de carte : actions communes + zone danger isolée =====
     Dupliquer (action fréquente, accessible depuis n'importe quel onglet) ;
     Supprimer seul bouton rouge de la carte, séparé par un filet rouge —
     plus de mélange visuel avec les boutons anodins. */
  .carte-pied,
  .carte-danger {
    display: flex;
    gap: 0.4rem;
    margin-top: 0.35rem;
    padding-top: 0.45rem;
    border-top: 1px solid var(--bordure-ui);
  }
  .carte-danger {
    border-top-color: color-mix(in srgb, var(--message-error-color) 40%, transparent);
  }
  .carte-pied .action,
  .carte-danger .action {
    flex: 1;
    text-align: center;
  }

  /* ===== Barre d'onglets segmentée (piste encastrée, cellules égales) =====
     3 onglets = 1 rangée de 3 ; 4 (onglet Lecture vidéo) = grille 2x2.
     Recette VISUELLEMENT DISTINCTE des .fit-btn (options de valeur) : la
     navigation doit se lire comme une navigation, pas comme un bouton
     de plus. */
  .tabs {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 0.125rem;
    padding: 0.125rem;
    margin-bottom: 0.2rem;
    background: color-mix(in srgb, var(--fond-controle) 65%, transparent);
    border: 1px solid var(--bordure-ui);
    border-radius: var(--rayon-petit);
  }
  .tabs.quatre {
    grid-template-columns: repeat(2, 1fr);
  }
  .tab {
    min-width: 0;
    background: transparent;
    color: var(--texte);
    border: none;
    border-radius: calc(var(--rayon-petit) - 1px);
    opacity: 0.7;
    padding: 0.3rem 0.2rem;
    font: inherit;
    font-size: 0.72rem;
    line-height: 1.2;
    cursor: pointer;
    text-align: center;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    transition: background 0.15s ease, opacity 0.15s ease, box-shadow 0.15s ease;
  }
  .tab:hover {
    opacity: 0.95;
    background: var(--btn-surface-hover);
  }
  .tab.active {
    opacity: 1;
    font-weight: 600;
    background:
      linear-gradient(
        180deg,
        color-mix(in srgb, var(--accent-carte) 22%, var(--fond-controle)) 0%,
        color-mix(in srgb, var(--accent-carte) 10%, var(--fond-controle)) 100%
      );
    box-shadow: inset 0 -2px 0 var(--accent-carte);
  }

  /* ===== Boutons d'options (.fit-btn) : grille à cellules égales =====
     Recette propre, distincte des onglets : bordure + relief au repos,
     accent de la carte à l'état actif. Grille fixe → rangées régulières
     quel que soit le nombre de boutons (plus de flex-wrap déchiré). */
  .fit-buttons {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(4.6rem, 1fr));
    gap: 0.25rem;
  }
  .fit-btn {
    min-width: 0;
    background: var(--btn-surface);
    color: var(--texte);
    border: 1px solid var(--bordure);
    box-shadow: var(--btn-inset);
    opacity: 0.8;
    padding: 0.3rem 0.4rem;
    font: inherit;
    font-size: 0.75rem;
    line-height: 1.2;
    cursor: pointer;
    text-align: center;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    transition: background 0.15s ease, box-shadow 0.15s ease, border-color 0.15s ease, opacity 0.15s ease;
  }
  .fit-btn:hover {
    background: var(--btn-surface-hover);
    box-shadow: var(--btn-inset-hover);
    opacity: 1;
    border-color: var(--bordure-active);
  }
  .fit-btn.active {
    opacity: 1;
    font-weight: 600;
    border-color: var(--accent-carte);
    box-shadow: var(--btn-inset-hover), inset 0 -2px 0 var(--accent-carte);
    background:
      linear-gradient(
        180deg,
        color-mix(in srgb, var(--accent-carte) 18%, var(--fond-controle)) 0%,
        color-mix(in srgb, var(--accent-carte) 8%, var(--fond-controle)) 100%
      );
  }

  /* ===== Slider compact une ligne ===== */
  .slider-ligne {
    display: grid;
    grid-template-columns: auto 1fr auto;
    align-items: center;
    gap: 0.4rem;
    min-width: 0;
  }
  .slider-label {
    font-size: 0.7rem;
    opacity: 0.6;
    white-space: nowrap;
  }
  .slider-val {
    font-size: 0.72rem;
    opacity: 0.8;
    font-variant-numeric: tabular-nums;
    text-align: right;
    min-width: 2.5rem;
  }

  /* ===== Morphing ===== */
  .morph-bloc {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
    margin-top: 0.4rem;
    padding-top: 0.4rem;
    border-top: 1px solid var(--bordure-ui);
  }

  /* ===== Widget goal : liste des Creator Goals Twitch ===== */
  .goal-liste {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
  }
  .goal-item {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    text-align: left;
    background: var(--btn-surface);
    color: var(--texte);
    border: 1px solid var(--bordure);
    box-shadow: var(--btn-inset);
    padding: 0.4rem 0.5rem;
    font: inherit;
    cursor: pointer;
    transition: background 0.15s ease, border-color 0.15s ease, box-shadow 0.15s ease;
  }
  .goal-item:hover {
    background: var(--btn-surface-hover);
    border-color: var(--bordure-active);
  }
  /* Goal lié : même repère actif que .fit-btn.active (bord + liseret). */
  .goal-item.lie {
    border-color: var(--accent-carte);
    box-shadow: var(--btn-inset-hover), inset 0 -2px 0 var(--accent-carte);
  }
  .goal-item-haut {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    min-width: 0;
  }
  .goal-item-nom {
    flex: 1;
    font-size: 0.82rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .goal-item-badge {
    flex-shrink: 0;
    font-size: 0.62rem;
    padding: 0.05rem 0.3rem;
    border: 1px solid var(--accent-carte);
    color: var(--accent-carte);
    border-radius: 3px;
  }
  .goal-item-chiffres {
    font-size: 0.75rem;
    opacity: 0.75;
    font-variant-numeric: tabular-nums;
  }
  .goal-item-bar {
    height: 0.25rem;
    background: var(--fond-controle);
    overflow: hidden;
  }
  .goal-item-fill {
    display: block;
    height: 100%;
    background: var(--accent-carte);
    transition: width 0.3s ease;
  }
  input {
    background: var(--fond-controle);
    color: var(--texte);
    border: 1px solid var(--bordure-ui);
    padding: 0.35rem 0.5rem;
    font: inherit;
    font-size: 0.85rem;
    width: 100%;
  }
  input:focus {
    outline: none;
    border-color: var(--accent-carte);
  }
  /* Style select = global (app.css). Locaux : largeur pleine + accent
     par-carte au focus (pas l'accent-ui global). */
  select {
    width: 100%;
  }
  select:focus {
    border-color: var(--accent-carte);
  }
  .media-val {
    font-size: 0.75rem;
    opacity: 0.8;
    font-variant-numeric: tabular-nums;
  }
  .range {
    width: 100%;
    accent-color: var(--accent-carte);
    background: var(--fond);
    color: var(--texte);
  }

  /* ===== Carrousel d'images : liste ordonnée des slides ===== */
  .slides-list {
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
    max-height: 10rem;
    overflow-y: auto;
  }
  .slide-ligne {
    display: flex;
    align-items: center;
    gap: 0.25rem;
    background: var(--btn-surface);
    border: 1px solid var(--bordure);
    box-shadow: var(--btn-inset);
    padding: 0.2rem 0.3rem;
    min-width: 0;
  }
  .slide-nom {
    flex: 1;
    font-size: 0.75rem;
    opacity: 0.85;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }
  .slide-btn {
    flex-shrink: 0;
    background: var(--fond-controle);
    color: var(--texte);
    border: 1px solid var(--bordure-ui);
    font: inherit;
    font-size: 0.7rem;
    line-height: 1;
    padding: 0.2rem 0.35rem;
    cursor: pointer;
    min-width: 1.4rem;
    text-align: center;
  }
  .slide-btn:hover {
    background: var(--btn-surface-hover);
    border-color: var(--bordure-active);
  }
  .slide-btn:disabled {
    opacity: 0.3;
    cursor: default;
  }
  .slide-del {
    color: var(--dash-danger);
  }
  /* ===== Speedrun ===== */
  .speedrun-ctrl-row {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(3.625rem, 1fr));
    gap: 0.25rem;
  }
  .speedrun-ctrl-row .action {
    text-align: center;
    padding: 0.3rem 0.3rem;
    font-size: 0.75rem;
    min-height: 1.8rem;
    line-height: 1.1;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .speedrun-ctrl-row .action:disabled {
    opacity: 0.35;
    cursor: default;
  }
  .speedrun-start {
    --btn-tint: var(--dash-success);
    --btn-border: linear-gradient(var(--dash-success), var(--dash-success));
    color: var(--dash-success);
  }
  .speedrun-stop {
    --btn-tint: var(--dash-danger);
    --btn-border: linear-gradient(var(--dash-danger), var(--dash-danger));
    color: var(--dash-danger);
  }
  .speedrun-toggles {
    display: flex;
    gap: 0.25rem;
    justify-content: center;
    flex-wrap: wrap;
  }
  .setting-toggle {
    display: flex;
    align-items: center;
    gap: 0.25rem;
    padding: 0.125rem 0.375rem;
    border-radius: var(--rayon-petit);
    border: 1px solid var(--bordure-ui);
    background: var(--btn-surface);
    box-shadow: var(--btn-inset);
    cursor: pointer;
    font-size: 0.72rem;
    transition: background 0.15s ease, box-shadow 0.15s ease, border-color 0.15s ease;
  }
  .setting-toggle:hover {
    background: var(--btn-surface-hover);
    box-shadow: var(--btn-inset-hover);
    border-color: var(--bordure-ui-active);
  }
  .setting-toggle.active {
    border-color: var(--accent-carte);
    background: color-mix(in srgb, var(--accent-carte) 10%, var(--fond-controle));
  }
  .setting-toggle input {
    margin: 0;
  }
</style>
