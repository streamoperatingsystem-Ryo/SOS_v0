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
    setMediaPixelLocal, resetMedia, clearWidgetMedia, commitScene,
    importFond, setBgFit, setBgZoomLocal, setBgRotLocal, setBgOffsetXLocal,
    setBgOffsetYLocal, setBgLumLocal, setBgContrasteLocal, setBgTeinteLocal,
    setBgFlouLocal, setBgPixelLocal, resetFond, clearFond,
    setWidgetMediaPaused, setWidgetMediaTime, setBgPaused, setBgTime,
    setWidgetTrou, deleteObsTrouSource,
    setChatFiltre, setChatTaillePolice, setSpeedrunPolice, setSpeedrunConfig,
    setInputViewerConfig, setCameraDevice,
    resetMorphsWidget, resetMorphsFond,
  } from "../stores/scene";
  import { morphMode, morphIntensite, morphRayon, morphSens } from "../stores/morph";
  import { videoRegistry } from "../stores/video";
  import { carteEditionCible, confirmDeleteWidget, titreFondModalOpen } from "../stores/ui";
  import { type ChatFiltre } from "../stores/chat";
  import {
    phaseTimer, totalSplits, segmentsLss,
    nomJeuLss, nomCategorieLss, estActif, estConnecte, nomProcessus,
    cheminAsl, cheminLss, settingsAsl, erreurSpeedrun,
    chargerAsl, chargerLss, demarrerSpeedrun, arreterSpeedrun, majSettings,
    actionManuelle, sauverPreferencesSpeedrun,
  } from "../stores/speedrun";
  import { tauri } from "../tauri";
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
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
  let tabWidget = $state<"contenu" | "transformation" | "effets" | "lecture">("contenu");
  let tabFond = $state<"contenu" | "transformation" | "effets" | "lecture">("contenu");
  // Réinitialise à "contenu" quand on change de widget sélectionné.
  $effect(() => {
    void selectedId;
    tabWidget = "contenu";
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
  let isMediaWidget = $derived(selectedType === "media");
  let chatFiltreVal = $derived(selectedWidget?.chatFiltre ?? "unifie");
  let chatTaille = $derived(selectedWidget?.taillePolice ?? 16);

  // id court = 8 premiers caractères
  let shortId = $derived(selectedId ? selectedId.slice(0, 8) : "");

  // ===== Label du widget pour l'en-tête =====
  // Affiche le type lisible du widget (ex: "Splitter", "Chat", "Caméra") au
  // lieu de l'ID technique. Si l'utilisateur a défini un titre personnalisé
  // (champ titre du widget), on l'utilise en priorité. Pour les widgets média,
  // on ajoute le nom du fichier importé si disponible (mediaNom).
  const TYPE_LABELS: Record<string, string> = {
    media: "Média",
    chat: "Chat",
    camera: "Caméra",
    "welcome-clip": "Clip de bienvenue",
    "input-viewer": "Input Viewer",
    speedrun: "Splitter",
  };
  let widgetLabel = $derived.by(() => {
    // Titre perso prioritaire.
    if (selectedWidget?.titre && selectedWidget.titre.length > 0) {
      return selectedWidget.titre;
    }
    const typeLabel = TYPE_LABELS[selectedWidget?.type ?? "media"] ?? "Widget";
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
  let selectedKind = $derived(selectedWidget?.kind ?? "image");
  let selectedTrou = $derived(selectedWidget?.trou === true);
  let selectedObsSource = $derived(selectedWidget?.obsSource ?? null);
  let widgetMediaPaused = $derived(selectedWidget?.mediaPaused ?? true);
  let widgetMediaTime = $derived(selectedWidget?.mediaTime ?? 0);
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
  let hasFond = $derived(bgMedia.length > 0);
  let hasBgTitre = $derived(($sceneStore.bgTitre ?? "").length > 0);
  let fondVideoEl = $derived($videoRegistry.get("fond"));
  let bgPaused = $derived($sceneStore.bgPaused ?? true);
  let bgTime = $derived($sceneStore.bgTime ?? 0);

  // ===== Cadres SVG =====
  // (cadreWidget/cadreApp non utilisés ici — l'activation se fait dans la
  // section Personnalisation de Toolbar.)

  // ===== Modes d'affichage (fit) =====
  const FIT_MODES = ["ajuster", "remplir", "etendre", "etirer", "centrer", "vignette"];
  const FIT_LABELS: Record<string, string> = {
    ajuster: "Ajuster", remplir: "Remplir", etendre: "Étendre",
    etirer: "Étirer", centrer: "Centrer", vignette: "Vignette",
  };
  function labelFit(mode: string): string { return FIT_LABELS[mode] ?? mode; }

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
  async function onResetMedia() { if (selectedId) await resetMedia(selectedId); }
  async function onClearMedia() { if (selectedId) await clearWidgetMedia(selectedId); }
  function onWidgetTogglePlay() { if (selectedId) setWidgetMediaPaused(selectedId, !widgetMediaPaused); }
  function onWidgetSeek(t: number) { if (selectedId) setWidgetMediaTime(selectedId, t); }
  function onToggleTrou() { if (selectedId) setWidgetTrou(selectedId, !selectedTrou); }
  function onSupprimerClick() { confirmDeleteWidget.set(true); }

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
  async function onBgReset() { await resetFond(); }
  async function onResetMorphsFond() { await resetMorphsFond(); }
  function onBgTogglePlay() { setBgPaused(!bgPaused); }
  function onBgSeek(t: number) { setBgTime(t); }

  // ===== Chat widget =====
  const FILTRES: ChatFiltre[] = ["unifie", "twitch", "youtube", "kick", "tiktok"];
  function onFiltreClick(f: ChatFiltre) { if (selectedId) setChatFiltre(selectedId, f); }
  function onTailleInput(e: Event) { if (selectedId) setChatTaillePolice(selectedId, parseFloat((e.target as HTMLInputElement).value)); }

  // ===== Pop-out chat =====
  let popoutOpen = $state(false);
  onMount(async () => {
    await listen("popout-closed", () => { popoutOpen = false; });
  });
  async function onDetacher() {
    try { popoutOpen = await tauri.chatPopoutToggle(); } catch (e) { console.warn("onDetacher:", e); }
  }

  // ===== Widget speedrun =====
  let srPoliceVal = $derived(selectedWidget?.srPolice ?? "Rajdhani");
  let srTailleVal = $derived(selectedWidget?.srTaillePolice ?? 27);
  function onSrPoliceChange(e: Event) { if (!selectedId) return; const p = (e.target as HTMLSelectElement).value; setSpeedrunPolice(selectedId, p, srTailleVal); }
  function onSrTailleInput(e: Event) { if (!selectedId) return; setSpeedrunPolice(selectedId, srPoliceVal, parseFloat((e.target as HTMLInputElement).value)); }

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

  // ===== Widget input viewer =====
  let inputModeVal = $derived(selectedWidget?.inputMode ?? "clavier");
  let inputLayoutVal = $derived(selectedWidget?.inputLayout ?? "azerty");
  let inputCouleurVal = $derived(selectedWidget?.inputCouleur ?? "#8b5cf6");
  let inputSkinVal = $derived(selectedWidget?.inputSkin ?? "xbox");
  let bindingModalOpen = $state(false);
  let captureActive = $state(false);
  $effect(() => {
    tauri.inputViewerEtatActif().then((a) => (captureActive = a)).catch(() => {});
  });
  async function toggleCaptureInput() {
    try {
      await tauri.inputViewerSetActif(!captureActive);
      captureActive = !captureActive;
    } catch (e) { console.warn("toggleCaptureInput:", e); }
  }

  // ===== Widget caméra =====
  let cameraDeviceId = $derived(selectedWidget?.cameraDeviceId ?? null);
  let cameraDevices = $state<string[]>([]);
  $effect(() => {
    if (!isCameraWidget) { cameraDevices = []; return; }
    let cancelled = false;
    (async () => {
      try {
        const list = await tauri.pcEnumerateCameras();
        if (!cancelled) cameraDevices = list;
      } catch (e) {
        console.warn("pcEnumerateCameras:", e);
        if (!cancelled) cameraDevices = [];
      }
    })();
    return () => { cancelled = true; };
  });
  function onCameraDeviceChange(e: Event) {
    if (!selectedId) return;
    const val = (e.target as HTMLSelectElement).value;
    setCameraDevice(selectedId, val === "" ? null : val);
  }

  // ===== Source OBS sous SOS (trou) =====
  let obsDialogOpen = $state(false);
  function onOpenObsDialog() { if (!selectedId || !selectedTrou) return; obsDialogOpen = true; }
  function onCloseObsDialog() { obsDialogOpen = false; }
  async function onDeleteObsSource() { if (!selectedId) return; await deleteObsTrouSource(selectedId); }

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
      <button type="button" class="carte-entete" onclick={() => (ouverte = !ouverte)} aria-expanded={ouverte}>
        <span class="carte-fleche">{ouverte ? "▼" : "▶"}</span>
        <span class="carte-titre">
          {#if cible === "widget"}
            {widgetLabel}{#if shortId} · {shortId}{/if}
          {:else}
            Arrière-plan
          {/if}
        </span>
      </button>
      <button type="button" class="carte-fermer" onclick={() => carteEditionCible.set(null)} title="Fermer" aria-label="Fermer la carte d'édition">✕</button>
    </div>

    {#if ouverte}
      <div class="carte-corps">
        {#if cible === "widget"}
          <!-- ====== ÉDITION WIDGET ====== -->
          {#if !selectedId}
            <p class="hint">Aucun widget sélectionné.</p>
          {:else if isCameraWidget}
            <!-- Widget caméra : pas d'onglets -->
            {#if shortId}
              <label class="field">
                <span class="field-label">id</span>
                <input value={shortId} readonly spellcheck="false" />
              </label>
            {/if}
            <div class="field">
              <span class="field-label">Device caméra</span>
              <select value={cameraDeviceId ?? ""} onchange={onCameraDeviceChange}>
                <option value="">(device par défaut)</option>
                {#each cameraDevices as dev}
                  <option value={dev}>{dev}</option>
                {/each}
              </select>
            </div>
            {#if cameraDevices.length === 0}
              <p class="hint">Aucune caméra détectée. Vérifier la webcam ?</p>
            {/if}
          {:else}
            <!-- Onglets horizontaux -->
            <div class="tabs">
              <button class="tab" class:active={tabWidget === "contenu"} onclick={() => tabWidget = "contenu"}>Contenu</button>
              <button class="tab" class:active={tabWidget === "transformation"} onclick={() => tabWidget = "transformation"}>Transformation</button>
              <button class="tab" class:active={tabWidget === "effets"} onclick={() => tabWidget = "effets"}>Effets</button>
              {#if selectedKind === "video"}
                <button class="tab" class:active={tabWidget === "lecture"} onclick={() => tabWidget = "lecture"}>Lecture</button>
              {/if}
            </div>

            {#if tabWidget === "contenu"}
              {#if shortId}
                <label class="field">
                  <span class="field-label">id</span>
                  <input value={shortId} readonly spellcheck="false" />
                </label>
              {/if}
              {#if !isCameraWidget}
                <button class="action" onclick={importMedia}>Importer un média</button>
              {/if}
              {#if isChatWidget}
                <div class="fit-group">
                  <span class="field-label">Filtre chat</span>
                  <div class="fit-buttons">
                    {#each FILTRES as f}
                      <button class="fit-btn" class:active={chatFiltreVal === f} onclick={() => onFiltreClick(f)}>{f}</button>
                    {/each}
                  </div>
                </div>
                {@render sliderLigne("Taille police", chatTaille, 8, 48, 1, onTailleInput, () => {}, (v) => v + "px")}
                <button class="action" onclick={onDetacher}>
                  {popoutOpen ? "Réattacher" : "Détacher"}
                </button>
              {:else if isInputViewerWidget}
                <div class="fit-group">
                  <span class="field-label">Mode</span>
                  <div class="fit-buttons">
                    <button class="fit-btn" class:active={inputModeVal === "clavier"} onclick={() => selectedId && setInputViewerConfig(selectedId, { inputMode: "clavier" })}>Clavier</button>
                    <button class="fit-btn" class:active={inputModeVal === "souris"} onclick={() => selectedId && setInputViewerConfig(selectedId, { inputMode: "souris" })}>Souris</button>
                    <button class="fit-btn" class:active={inputModeVal === "numpad"} onclick={() => selectedId && setInputViewerConfig(selectedId, { inputMode: "numpad" })}>Numpad</button>
                    <button class="fit-btn" class:active={inputModeVal === "manette"} onclick={() => selectedId && setInputViewerConfig(selectedId, { inputMode: "manette" })}>Manette</button>
                  </div>
                </div>
                {#if inputModeVal === "clavier"}
                  <div class="fit-group">
                    <span class="field-label">Layout</span>
                    <div class="fit-buttons">
                      <button class="fit-btn" class:active={inputLayoutVal === "azerty"} onclick={() => selectedId && setInputViewerConfig(selectedId, { inputLayout: "azerty" })}>AZERTY</button>
                      <button class="fit-btn" class:active={inputLayoutVal === "qwerty"} onclick={() => selectedId && setInputViewerConfig(selectedId, { inputLayout: "qwerty" })}>QWERTY</button>
                    </div>
                  </div>
                {/if}
                {#if inputModeVal === "manette"}
                  <div class="fit-group">
                    <span class="field-label">Skin</span>
                    <div class="fit-buttons">
                      <button class="fit-btn" class:active={inputSkinVal === "xbox"} onclick={() => selectedId && setInputViewerConfig(selectedId, { inputSkin: "xbox" })}>Xbox</button>
                      <button class="fit-btn" class:active={inputSkinVal === "ps"} onclick={() => selectedId && setInputViewerConfig(selectedId, { inputSkin: "ps" })}>PlayStation</button>
                      <button class="fit-btn" class:active={inputSkinVal === "8bitdo"} onclick={() => selectedId && setInputViewerConfig(selectedId, { inputSkin: "8bitdo" })}>8BitDo</button>
                    </div>
                  </div>
                  <button class="action" onclick={() => (bindingModalOpen = true)}>Binder les touches</button>
                {/if}
                <div class="field">
                  <span class="field-label">Couleur touches</span>
                  <input type="color" value={inputCouleurVal} oninput={(e) => selectedId && setInputViewerConfig(selectedId, { inputCouleur: (e.target as HTMLInputElement).value })} />
                </div>
                <button class="action" onclick={toggleCaptureInput}>
                  {captureActive ? "Arrêter la capture" : "Démarrer la capture"}
                </button>
                <p class="hint">
                  {captureActive ? "Capture active : les entrées s'affichent côté diffusion." : "Capture arrêtée — aucun coût CPU."}
                </p>
              {:else if isSpeedrunWidget}
                {#if $erreurSpeedrun}
                  <p class="hint" style="color: var(--message-error-color);">⚠ {$erreurSpeedrun}</p>
                {/if}
                <div class="field">
                  <span class="field-label">Jeu</span>
                  <span class="media-val">
                    {#if $nomJeuLss || $nomCategorieLss}
                      {$nomJeuLss}{#if $nomCategorieLss} — {$nomCategorieLss}{/if}
                    {:else}
                      <span class="hint">Aucun splits chargé</span>
                    {/if}
                  </span>
                </div>
                {#if $estConnecte}
                  <p class="hint" style="color: var(--message-ok-color);">● Connecté{$nomProcessus ? ` (${$nomProcessus})` : ""}</p>
                {:else if $estActif}
                  <p class="hint" style="color: var(--message-user-action-color);">○ En attente…</p>
                {/if}
                <button class="action" onclick={onSpeedrunChoisirAsl}>
                  {#if $cheminAsl}✓ ASL chargé{:else}Charger .asl{/if}
                </button>
                <button class="action" onclick={onSpeedrunChoisirLss}>
                  {#if $cheminLss}✓ LSS chargé{:else}Charger .lss{/if}
                </button>
                <div class="speedrun-ctrl-row">
                  {#if !$estActif}
                    <button class="action speedrun-start" onclick={onSpeedrunDemarrer}>▶ Start</button>
                  {:else}
                    <button class="action speedrun-stop" onclick={onSpeedrunArreter}>■ Stop</button>
                  {/if}
                  <button class="action" onclick={() => actionManuelle("split")} disabled={!$estActif}>Split</button>
                  <button class="action" onclick={() => actionManuelle("skip")} disabled={!$estActif}>Skip</button>
                  <button class="action" onclick={() => actionManuelle("undo")} disabled={!$estActif}>Undo</button>
                  <button class="action" onclick={() => actionManuelle("reset")} disabled={!$estActif}>Reset</button>
                  <button class="action" onclick={() => actionManuelle("pause")} disabled={!$estActif}>
                    {#if $phaseTimer === "Paused"}Resume{:else}Pause{/if}
                  </button>
                </div>
                <div class="speedrun-toggles">
                  <label class="setting-toggle" class:active={srStartActif}>
                    <input type="checkbox" checked={srStartActif} onchange={onSpeedrunToggleStart} />
                    <span>Auto Start</span>
                  </label>
                  <label class="setting-toggle" class:active={srSplitActif}>
                    <input type="checkbox" checked={srSplitActif} onchange={onSpeedrunToggleSplit} />
                    <span>Auto Split</span>
                  </label>
                  <label class="setting-toggle" class:active={srResetActif}>
                    <input type="checkbox" checked={srResetActif} onchange={onSpeedrunToggleReset} />
                    <span>Auto Reset</span>
                  </label>
                </div>
                <div class="field">
                  <span class="field-label">Police</span>
                  <select class="police-select" value={srPoliceVal} onchange={onSrPoliceChange}>
                    {#each POLICES_TITRE as p (p.id)}
                      <option value={p.id} style="font-family: '{p.id}', sans-serif;">{p.nom}</option>
                    {/each}
                  </select>
                </div>
                {@render sliderLigne("Taille police", srTailleVal, 8, 48, 1, onSrTailleInput, () => {}, (v) => v + "px")}
              {:else}
                <!-- Widget média : fenêtre de capture + source OBS -->
                <button class="action" onclick={onToggleTrou}>
                  {selectedTrou ? "Désactiver la fenêtre de capture" : "Activer la fenêtre de capture"}
                </button>
                {#if selectedTrou}
                  {#if selectedObsSource}
                    <button class="action" onclick={onDeleteObsSource}>Supprimer la source OBS</button>
                  {:else}
                    <button class="action" onclick={onOpenObsDialog}>Lier une source OBS</button>
                  {/if}
                {/if}
              {/if}
            {:else if tabWidget === "transformation"}
              <Gizmo3D />
              {@render controlesTransformation()}
            {:else if tabWidget === "effets"}
              {@render controlesEffetsWidget()}
            {:else if tabWidget === "lecture"}
              {@render controlesLecture()}
            {/if}
            <button class="action" onclick={onSupprimerClick}>Supprimer le widget</button>
          {/if}
        {:else}
          <!-- ====== ÉDITION FOND ====== -->
          {#if !hasFond}
            <button class="action" onclick={importFond}>Importer un fond</button>
            <p class="hint">Aucun fond. Cliquer « Importer un fond ».</p>
          {:else}
            <div class="tabs">
              <button class="tab" class:active={tabFond === "contenu"} onclick={() => tabFond = "contenu"}>Contenu</button>
              <button class="tab" class:active={tabFond === "transformation"} onclick={() => tabFond = "transformation"}>Transformation</button>
              <button class="tab" class:active={tabFond === "effets"} onclick={() => tabFond = "effets"}>Effets</button>
              {#if bgKind === "video"}
                <button class="tab" class:active={tabFond === "lecture"} onclick={() => tabFond = "lecture"}>Lecture</button>
              {/if}
            </div>

            {#if tabFond === "contenu"}
              <button class="action" onclick={importFond}>Remplacer le fond</button>
              <button class="action" onclick={() => clearFond()}>Supprimer le fond</button>
              <button class="action" onclick={() => titreFondModalOpen.set(true)}>
                {hasBgTitre ? "Modifier le titre du fond" : "Ajouter un titre au fond"}
              </button>
            {:else if tabFond === "transformation"}
              <div class="fit-group">
                <span class="field-label">Affichage fond</span>
                <div class="fit-buttons">
                  {#each FIT_MODES as mode}
                    <button class="fit-btn" class:active={bgFit === mode} onclick={() => onBgFitClick(mode)}>{labelFit(mode)}</button>
                  {/each}
                </div>
              </div>
              {@render sliderLigne("Zoom", bgZoom, 0.2, 5, 0.1, onBgZoomInput, onBgZoomChange, (v) => v.toFixed(2))}
              {@render sliderLigne("Rotation", bgRot, -180, 180, 5, onBgRotInput, onBgRotChange, (v) => Math.round(v) + "°")}
              {@render sliderLigne("Offset X", bgOffsetX, -2000, 2000, 1, onBgOffsetXInput, onBgOffsetChange, (v) => Math.round(v) + "px")}
              {@render sliderLigne("Offset Y", bgOffsetY, -2000, 2000, 1, onBgOffsetYInput, onBgOffsetChange, (v) => Math.round(v) + "px")}
              <button class="action" onclick={onBgReset}>Reset transformation</button>
            {:else if tabFond === "effets"}
              {@render sliderLigne("Luminosité", bgLum, -100, 100, 1, onBgLumInput, onBgEffetChange, (v) => String(Math.round(v)))}
              {@render sliderLigne("Contraste", bgContraste, -100, 100, 1, onBgContrasteInput, onBgEffetChange, (v) => String(Math.round(v)))}
              {@render sliderLigne("Teinte", bgTeinte, 0, 360, 1, onBgTeinteInput, onBgEffetChange, (v) => Math.round(v) + "°")}
              {@render sliderLigne("Flou", bgFlou, 0, 20, 0.5, onBgFlouInput, onBgEffetChange, (v) => v.toFixed(1) + "px")}
              {@render sliderLigne("Pixelisation", bgPixel, 0, 50, 1, onBgPixelInput, onBgEffetChange, (v) => String(Math.round(v)))}
              {#if bgKind === "image"}
                <div class="fit-group">
                  <span class="field-label">Morphing</span>
                  <div class="fit-buttons">
                    <button class="fit-btn" class:active={$morphMode} onclick={() => morphMode.update((m) => !m)}>Morphing</button>
                  </div>
                </div>
                {#if $morphMode}
                  <div class="morph-bloc">
                    <div class="fit-buttons">
                      <button class="fit-btn" class:active={$morphSens === "agrandir"} onclick={() => morphSens.set("agrandir")}>Agrandir</button>
                      <button class="fit-btn" class:active={$morphSens === "retrecir"} onclick={() => morphSens.set("retrecir")}>Rétrécir</button>
                    </div>
                    {@render sliderLigne("Intensité", $morphIntensite, 10, 200, 5, (e) => morphIntensite.set(+(e.target as HTMLInputElement).value), () => {}, (v) => v + "%")}
                    {@render sliderLigne("Rayon", Math.round($morphRayon * 100), 5, 50, 1, (e) => morphRayon.set(+(e.target as HTMLInputElement).value / 100), () => {}, (v) => v + "%")}
                    <button class="action" onclick={onResetMorphsFond}>Réinitialiser le morphing</button>
                    <p class="hint">Cliquez sur le fond (canvas) pour déformer. Échap = quitter.</p>
                  </div>
                {/if}
              {/if}
            {:else if tabFond === "lecture"}
              {#if bgKind === "video"}
                <PlayerBar videoEl={fondVideoEl} mediaPaused={bgPaused} mediaTime={bgTime} onTogglePlay={onBgTogglePlay} onSeek={onBgSeek} />
              {/if}
            {/if}
          {/if}
        {/if}
      </div>
    {/if}
  </div>
{/if}

{#if obsDialogOpen && selectedId}
  <ObsSourceDialog widgetId={selectedId} onClose={onCloseObsDialog} />
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

{#snippet controlesTransformation()}
  <div class="fit-group">
    <span class="field-label">Affichage</span>
    <div class="fit-buttons">
      {#each FIT_MODES as mode}
        <button class="fit-btn" class:active={currentFit === mode} onclick={() => onFitClick(mode)}>{labelFit(mode)}</button>
      {/each}
    </div>
  </div>
  {@render sliderLigne("Zoom", currentZoom, 0.2, 5, 0.1, onZoomInput, onZoomChange, (v) => v.toFixed(2))}
  {@render sliderLigne("Rotation", currentRot, -180, 180, 5, onRotInput, onRotChange, (v) => Math.round(v) + "°")}
  {@render sliderLigne("Offset X", currentOffsetX, -2000, 2000, 1, onOffsetXInput, onOffsetChange, (v) => Math.round(v) + "px")}
  {@render sliderLigne("Offset Y", currentOffsetY, -2000, 2000, 1, onOffsetYInput, onOffsetChange, (v) => Math.round(v) + "px")}
  <button class="action" onclick={onResetMedia}>Reset transformation</button>
  {#if selectedWidget?.media}
    <button class="action" onclick={onClearMedia}>Retirer le média</button>
  {/if}
{/snippet}

{#snippet controlesEffetsWidget()}
  {@render sliderLigne("Luminosité", currentLum, -100, 100, 1, onLumInput, onEffetChange, (v) => String(Math.round(v)))}
  {@render sliderLigne("Contraste", currentContraste, -100, 100, 1, onContrasteInput, onEffetChange, (v) => String(Math.round(v)))}
  {@render sliderLigne("Teinte", currentTeinte, 0, 360, 1, onTeinteInput, onEffetChange, (v) => Math.round(v) + "°")}
  {@render sliderLigne("Flou", currentFlou, 0, 20, 0.5, onFlouInput, onEffetChange, (v) => v.toFixed(1) + "px")}
  {@render sliderLigne("Pixelisation", currentPixel, 0, 50, 1, onPixelInput, onEffetChange, (v) => String(Math.round(v)))}
  {#if isMediaWidget && selectedKind === "image" && selectedWidget?.media}
    <div class="fit-group">
      <span class="field-label">Morphing</span>
      <div class="fit-buttons">
        <button class="fit-btn" class:active={$morphMode} onclick={() => morphMode.update((m) => !m)}>Morphing</button>
      </div>
    </div>
    {#if $morphMode}
      <div class="morph-bloc">
        <div class="fit-buttons">
          <button class="fit-btn" class:active={$morphSens === "agrandir"} onclick={() => morphSens.set("agrandir")}>Agrandir</button>
          <button class="fit-btn" class:active={$morphSens === "retrecir"} onclick={() => morphSens.set("retrecir")}>Rétrécir</button>
        </div>
        {@render sliderLigne("Intensité", $morphIntensite, 10, 200, 5, (e) => morphIntensite.set(+(e.target as HTMLInputElement).value), () => {}, (v) => v + "%")}
        {@render sliderLigne("Rayon", Math.round($morphRayon * 100), 5, 50, 1, (e) => morphRayon.set(+(e.target as HTMLInputElement).value / 100), () => {}, (v) => v + "%")}
        <button class="action" onclick={() => selectedId && resetMorphsWidget(selectedId)}>Réinitialiser le morphing</button>
        <p class="hint">Cliquez sur le média pour déformer (effets cumulés). Échap = quitter.</p>
      </div>
    {/if}
  {/if}
{/snippet}

{#snippet controlesLecture()}
  {#if selectedKind === "video"}
    <PlayerBar videoEl={widgetVideoEl} mediaPaused={widgetMediaPaused} mediaTime={widgetMediaTime} onTogglePlay={onWidgetTogglePlay} onSeek={onWidgetSeek} />
  {:else}
    <p class="hint">Pas de vidéo en lecture.</p>
  {/if}
{/snippet}

<style>
  .carte-edition {
    display: flex;
    flex-direction: column;
    width: 100%;
    min-width: 0;
    border-radius: var(--rayon);
    border: 2px dashed var(--accent-carte);
    background: color-mix(in srgb, var(--accent-carte) 22%, var(--fond-panneau));
    box-shadow: 0 0 16px var(--accent-carte-glow), 0 8px 24px rgba(0, 0, 0, 0.4);
    overflow: hidden;
    flex-shrink: 0;
    /* Tous les boutons internes héritent la teinte contextuelle de la carte
       (bordeaux widget / orange fond / jaune input-viewer) via la recette
       Aero --btn-*. */
    --btn-tint: var(--accent-carte);
  }

  /* Cible widget = bordeaux foncé (pointillés + glow, palette --dash-*) */
  .cible-widget {
    --accent-carte: var(--dash-danger);
    --accent-carte-glow: rgba(127, 29, 29, 0.45);
  }

  /* Cible fond = orange foncé (pointillés + glow, palette --dash-*) */
  .cible-fond {
    --accent-carte: var(--dash-orange);
    --accent-carte-glow: rgba(154, 52, 18, 0.4);
  }

  /* Cible input-viewer = jaune foncé (pointillés + glow, palette --dash-*) */
  .cible-input-viewer {
    --accent-carte: var(--dash-jaune);
    --accent-carte-glow: rgba(161, 98, 7, 0.4);
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
    width: 18px;
    height: 18px;
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
    max-height: 60vh;
    min-width: 0;
    overflow-y: auto;
    scrollbar-width: thin;
    scrollbar-color: var(--accent-carte-glow) transparent;
  }

  /* ===== Onglets ===== */
  .tabs {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 0.2rem;
    margin-bottom: 0.2rem;
  }
  .tab {
    background: var(--btn-surface);
    color: var(--texte);
    border: 1px solid var(--bordure);
    box-shadow: var(--btn-inset);
    opacity: 0.4;
    padding: 0.3rem 0.4rem;
    font: inherit;
    font-size: 0.78rem;
    cursor: pointer;
    text-align: center;
    transition: background 0.15s ease, box-shadow 0.15s ease, border-color 0.15s ease, opacity 0.15s ease;
  }
  .tab:hover {
    background: var(--btn-surface-hover);
    box-shadow: var(--btn-inset-hover);
    opacity: 0.7;
    border-color: var(--bordure-active);
  }
  .tab.active {
    opacity: 1;
    border-color: var(--accent-carte);
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
    border-top: 1px solid var(--bordure);
  }

  .action {
    /* Recette « verre Aero HUD » — bordure + surface teintées par la couleur
       contextuelle de la carte (--btn-tint = --accent-carte posé sur
       .carte-edition) dès l'état repos, pas seulement au hover/active.
       Bordure = couleur PLEINE de la carte (pas de mix avec --bordure qui la
       rendrait indistincte). Surface renforcée localement pour que la teinte
       soit visible (les --dash-* sont déjà assombries → le tint global 22% est
       trop faible). Utilise --btn-tint (pas --accent-carte directement) pour
       que les surcharges .speedrun-start/.speedrun-stop fonctionnent. */
    --btn-border: linear-gradient(var(--btn-tint), var(--btn-tint));
    --btn-surface:
      linear-gradient(
        180deg,
        color-mix(in srgb, var(--btn-tint) 38%, var(--fond-controle)) 0%,
        color-mix(in srgb, var(--btn-tint) 20%, var(--fond-controle)) 50%,
        var(--fond-controle) 100%
      );
    --btn-surface-hover:
      linear-gradient(
        180deg,
        color-mix(in srgb, var(--btn-tint) 50%, var(--fond-controle)) 0%,
        color-mix(in srgb, var(--btn-tint) 28%, var(--fond-controle)) 50%,
        var(--fond-controle) 100%
      );
    background:
      var(--btn-surface) padding-box,
      var(--btn-border) border-box;
    color: var(--texte);
    border: 1px solid transparent;
    box-shadow: var(--btn-inset);
    padding: 0.35rem 0.6rem;
    font: inherit;
    cursor: pointer;
    text-align: left;
    /* flex-shrink: 0 — les .action sont des enfants de .carte-corps (flex
       column, max-height: 60vh). Le global button { overflow: hidden }
       (app.css) fait que min-height: auto se résout à 0 (spec Flexbox :
       overflow != visible → min-size: auto = 0). Sans flex-shrink: 0, les
       boutons pleine largeur s'écrasent verticalement quand le contenu
       dépasse 60vh. */
    flex-shrink: 0;
    transition: box-shadow 0.2s ease, background 0.2s ease;
  }
  .action:hover {
    /* Hover : glow couleur contextuelle de la carte + surface illuminée. */
    background:
      var(--btn-surface-hover) padding-box,
      var(--btn-border) border-box;
    box-shadow: var(--btn-inset-hover), 0 0 8px var(--accent-carte-glow), 0 0 0 1px color-mix(in srgb, var(--accent-carte) 30%, transparent);
  }
  .action:active {
    background:
      linear-gradient(180deg, var(--fond-controle) 0%, color-mix(in srgb, var(--btn-tint) 8%, var(--fond-controle)) 100%) padding-box,
      var(--btn-border) border-box;
    box-shadow: inset 0 2px 4px rgba(0, 0, 0, 0.4);
  }
  .action:disabled {
    opacity: 0.35;
    cursor: not-allowed;
    box-shadow: none;
  }
  .action:disabled:hover {
    background:
      var(--btn-surface) padding-box,
      var(--btn-border) border-box;
    box-shadow: none;
  }

  .hint {
    font-size: 0.8rem;
    opacity: 0.6;
    line-height: 1.3;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
  }
  .field-label {
    font-size: 0.7rem;
    opacity: 0.6;
  }
  input {
    background: var(--fond-controle);
    color: var(--texte);
    border: 1px solid var(--bordure);
    padding: 0.35rem 0.5rem;
    font: inherit;
    font-size: 0.85rem;
    width: 100%;
  }
  input:focus {
    outline: none;
    border-color: var(--accent-carte);
  }
  input[readonly] {
    opacity: 0.6;
    cursor: default;
  }
  select {
    background: var(--fond-controle);
    color: var(--texte);
    border: 1px solid var(--bordure);
    padding: 0.35rem 0.5rem;
    font: inherit;
    font-size: 0.85rem;
    width: 100%;
  }
  select:focus {
    outline: none;
    border-color: var(--accent-carte);
  }
  .fit-group {
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
  }
  .fit-buttons {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 0.25rem;
  }
  .fit-btn {
    background: var(--btn-surface);
    color: var(--texte);
    border: 1px solid var(--bordure);
    box-shadow: var(--btn-inset);
    opacity: 0.4;
    padding: 0.3rem 0.4rem;
    font: inherit;
    font-size: 0.8rem;
    cursor: pointer;
    text-align: center;
    transition: background 0.15s ease, box-shadow 0.15s ease, border-color 0.15s ease, opacity 0.15s ease;
  }
  .fit-btn:hover {
    background: var(--btn-surface-hover);
    box-shadow: var(--btn-inset-hover);
    opacity: 0.7;
    border-color: var(--bordure-active);
  }
  .fit-btn.active {
    opacity: 1;
    border-color: var(--accent-carte);
    background:
      linear-gradient(
        180deg,
        color-mix(in srgb, var(--accent-carte) 18%, var(--fond-controle)) 0%,
        color-mix(in srgb, var(--accent-carte) 8%, var(--fond-controle)) 100%
      );
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
  .police-select {
    width: 100%;
    padding: 0.4rem 0.5rem;
    background: var(--fond-controle);
    color: var(--texte);
    border: 1px solid var(--bordure);
    font: inherit;
    font-size: 0.85rem;
    cursor: pointer;
  }
  .police-select:focus {
    outline: none;
    border-color: var(--accent-carte);
  }
  .police-select option {
    padding: 0.3rem;
    background: var(--fond);
    color: var(--texte);
  }

  /* ===== Speedrun ===== */
  .speedrun-ctrl-row {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(58px, 1fr));
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
    gap: 4px;
    justify-content: center;
    flex-wrap: wrap;
  }
  .setting-toggle {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 2px 6px;
    border-radius: var(--rayon-petit);
    border: 1px solid var(--bordure);
    background: var(--btn-surface);
    box-shadow: var(--btn-inset);
    cursor: pointer;
    font-size: 0.72rem;
    transition: background 0.15s ease, box-shadow 0.15s ease, border-color 0.15s ease;
  }
  .setting-toggle:hover {
    background: var(--btn-surface-hover);
    box-shadow: var(--btn-inset-hover);
    border-color: var(--bordure-active);
  }
  .setting-toggle.active {
    border-color: var(--accent-carte);
    background: color-mix(in srgb, var(--accent-carte) 10%, var(--fond-controle));
  }
  .setting-toggle input {
    margin: 0;
  }
</style>
