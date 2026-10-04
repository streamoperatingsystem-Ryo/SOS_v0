<script lang="ts">
  // Modale "Interactions chat" — tout ce qui est déclenché par le chat :
  //   (1) onglet Clip de bienvenue : attribution followers→clips, file +
  //       contrôles, config overlay (mini-canvas drag/resize),
  //   (2) onglet Bandeau premier message : config globale,
  //   (3) onglet Alertes : config par type (follow/raid/sub/resub/subgift/
  //       bits) + position overlay (migré depuis la sidebar Toolbar).
  //   (Commandes chat : prévues, pas encore implémentées — pas d'onglet.)
  import { followers, chargerFollowers, broadcaster } from "../stores/communaute";
  import {
    welcomeEtat,
    welcomeRegistre,
    welcomeSauverViewer,
    welcomeSupprimerViewer,
    welcomeAttribuerAuto,
    welcomeAttributionProgress,
    welcomeStop,
    welcomeSkip,
    welcomeRetirer,
    welcomeRemonter,
    welcomeDescendre,
    welcomeVider,
    welcomeResetSession,
    welcomeSetConfigGlobaleActif,
    welcomeTesterClip,
    welcomeSetDureeAffichage,
  } from "../stores/welcome";
  import {
    bandeauEtat,
    bandeauSetActif,
    bandeauSetDuree,
    bandeauSetPosition,
    bandeauSetPolice,
    bandeauResetSession,
    bandeauTester,
  } from "../stores/bandeau";
  import { POLICES_TITRE } from "../fonts";
  import {
    TYPES_ALERTE,
    type TypeAlerte,
    alertesConfig,
    sauverConfigType,
    configType,
  } from "../stores/alertes";
  import {
    positionOverlay,
    sauverPositionOverlay,
  } from "../stores/positionOverlay";
  import {
    commandesStore,
    chargerCommandes,
    sauverCommande,
    supprimerCommande,
    testerCommande,
  } from "../stores/commandes";
  import {
    padStore,
    padErreur,
    chargerPad,
    padSetActif,
    padSetTouche,
    padSupprimerTouche,
    padSetPlage,
    padTesterTouche,
    padImporterMedia,
    padImporterSon,
    DISPOSITION_PAD,
    TOUCHES_RESERVEES,
    NOMS_AFFICHAGE,
    NB_PLAGES,
  } from "../stores/padNumerique";
  import { sceneStore } from "../stores/scene";
  import { t } from "../stores/langue";
  import { get } from "svelte/store";
  import {
    tauri,
    type WelcomeViewerConfig,
    type WelcomeClipInfo,
    type AlerteTypeConfig,
    type CommandeConfig,
    type PositionOverlayConfig,
    type ToucheConfig,
  } from "../tauri";
  import Modal from "./Modal.svelte";

  let { onFermer = () => {} }: { onFermer?: () => void } = $props();

  // Onglet actif.
  let tab = $state<
    "bienvenue" | "bandeau" | "alertes" | "commandes" | "pad" | "position"
  >("bienvenue");

  // Section sélectionnée dans les onglets master-detail (liste gauche).
  let secBienvenue = $state<"general" | "file" | "attribution">("general");
  let secBandeau = $state<
    "general" | "duree" | "position" | "test" | "apparence"
  >("general");

  // ===== Alertes (config par type + overlay) =====
  const NOMS_ALERTE: Record<string, string> = {
    follow: "Follow",
    raid: "Raid",
    sub: "Sub",
    resub: "Resub",
    subgift: "Sub gift",
    bits: "Bits",
  };
  const PRESETS_OVERLAY: Record<string, string> = {
    "haut-gauche": "↖",
    "haut-centre": "↑",
    "haut-droite": "↗",
    "bas-gauche": "↙",
    "bas-centre": "↓",
    "bas-droite": "↘",
  };
  let alertes = $derived($alertesConfig);

  // Type d'alerte sélectionné dans la liste (panneau de détail à droite).
  let typeActif = $state<TypeAlerte>("follow");
  let cfgSel = $derived(cfgType(typeActif));

  function cfgType(ty: TypeAlerte): AlerteTypeConfig {
    return configType(alertes, ty);
  }
  function majAlerte(ty: TypeAlerte, patch: Partial<AlerteTypeConfig>) {
    void sauverConfigType(ty, { ...cfgType(ty), ...patch });
  }
  function toggleAlerteActif(ty: TypeAlerte) {
    majAlerte(ty, { actif: !cfgType(ty).actif });
  }
  async function testerAlerte(ty: TypeAlerte) {
    try {
      await tauri.alertesTester(ty);
    } catch {
    }
  }
  async function importerSonAlerte(ty: TypeAlerte) {
    try {
      const rel = await tauri.importSon();
      if (rel) majAlerte(ty, { son: rel });
    } catch (e) {
      alert(get(t)("commun.importRefuse", { erreur: String(e) }));
    }
  }
  // Mode média effectif du type : "aucun" | "image" | "video".
  function modeMedia(ty: TypeAlerte): "aucun" | "image" | "video" {
    const cfg = cfgType(ty);
    if (cfg.media && cfg.media_kind === "video") return "video";
    if (cfg.media) return "image";
    return "aucun";
  }
  async function importerMediaAlerte(ty: TypeAlerte, kind: "image" | "video") {
    try {
      const rel = await tauri.importAlerteMedia(kind);
      if (rel) majAlerte(ty, { media: rel, media_kind: kind });
    } catch (e) {
      alert(get(t)("commun.importRefuse", { erreur: String(e) }));
    }
  }

  // ===== Commandes chat (!commande → overlay diffusion) =====
  // Même organisation que les alertes : liste à gauche, détail à droite.
  let commandes = $derived($commandesStore);
  let cmdSelId = $state<string | null>(null);
  let cmdSel = $derived(commandes.find((c) => c.id === cmdSelId) ?? null);

  function commandeVierge(): CommandeConfig {
    return {
      id: "",
      actif: true,
      commande: "",
      duree_ms: 5000,
      texte_template: get(t)("interactions.commandes.defautTemplate"),
      taille_px: 48,
      cooldown_viewer_s: 0,
      cooldown_global_s: 0,
      son: undefined,
      media: undefined,
      media_kind: undefined,
    };
  }
  async function onAjouterCommande() {
    try {
      // Nom par défaut unique (commande1, commande2, …) — le Rust refuse les
      // noms vides ; le user renomme ensuite via le champ du panneau détail.
      const existants = new Set($commandesStore.map((c) => c.commande));
      let i = 1;
      while (existants.has(`commande${i}`)) i++;
      const st = await sauverCommande({ ...commandeVierge(), commande: `commande${i}` });
      if (st) cmdSelId = st.id;
    } catch (e) {
      console.error("ajouterCommande:", e);
    }
  }
  function majCommande(patch: Partial<CommandeConfig>) {
    if (!cmdSel) return;
    void sauverCommande({ ...cmdSel, ...patch });
  }
  async function importerMediaCommande(kind: "image" | "video") {
    try {
      const rel = await tauri.importAlerteMedia(kind);
      if (rel) majCommande({ media: rel, media_kind: kind });
    } catch (e) {
      alert(get(t)("commun.importRefuse", { erreur: String(e) }));
    }
  }
  async function importerSonCommande() {
    try {
      const rel = await tauri.importSon();
      if (rel) majCommande({ son: rel });
    } catch (e) {
      alert(get(t)("commun.importRefuse", { erreur: String(e) }));
    }
  }
  // Mode média effectif de la commande : "aucun" | "image" | "video".
  function modeMediaCommande(c: CommandeConfig): "aucun" | "image" | "video" {
    if (c.media && c.media_kind === "video") return "video";
    if (c.media) return "image";
    return "aucun";
  }

  // ===== Pad numérique (16 touches Numpad × 3 plages → overlay diffusion) =====
  let pad = $derived($padStore);
  let padActif = $derived(pad?.actif ?? false);
  let padPlage = $derived(pad?.plage_actuelle ?? 0);
  let padErr = $derived($padErreur);

  /// Touche sélectionnée dans la grille (pour le panneau de détail).
  let padSelCode = $state<string | null>(null);

  /// Touche courante (config de la touche sélectionnée à la plage courante).
  let padSel = $derived(
    pad && padSelCode
      ? pad.touches[`${padSelCode}_${padPlage}`] ?? null
      : null,
  );

  /// Mode média effectif de la touche : "aucun" | "audio" | "image" | "video".
  function modeMediaPad(tc: ToucheConfig | null): "aucun" | "audio" | "image" | "video" {
    if (!tc) return "aucun";
    if (tc.media_type === "audio") return "audio";
    if (tc.media_type === "video") return "video";
    if (tc.media_type === "image") return "image";
    return "aucun";
  }

  /// Met à jour un champ de la touche sélectionnée (sauvegarde immédiate).
  function majPad(patch: Partial<ToucheConfig>): void {
    if (!padSelCode || !padSel) return;
    void padSetTouche(padSelCode, padPlage, { ...padSel, ...patch });
  }

  /// Importe un média (image OU vidéo) pour la touche sélectionnée.
  async function onImporterMediaPad(kind: "image" | "video"): Promise<void> {
    if (!padSelCode) return;
    try {
      await padImporterMedia(padSelCode, padPlage, kind);
    } catch (e) {
      alert(get(t)("commun.importRefuse", { erreur: String(e) }));
    }
  }

  /// Importe un son pour la touche sélectionnée (audio pur OU son d'image).
  async function onImporterSonPad(): Promise<void> {
    if (!padSelCode) return;
    try {
      await padImporterSon(padSelCode, padPlage);
    } catch (e) {
      alert(get(t)("commun.importRefuse", { erreur: String(e) }));
    }
  }

  /// Retire le média de la touche sélectionnée.
  function onRetirerMediaPad(): void {
    if (!padSel) return;
    majPad({ media: undefined, media_kind: undefined, media_type: undefined });
  }

  /// Retire le son de la touche sélectionnée.
  function onRetirerSonPad(): void {
    if (!padSel) return;
    majPad({ son: undefined });
  }

  /// Supprime (reset) la touche sélectionnée.
  function onSupprimerTouchePad(): void {
    if (!padSelCode) return;
    void padSupprimerTouche(padSelCode, padPlage);
  }

  /// Test manuel : déclenche la touche sélectionnée côté diffusion.
  function onTesterTouchePad(): void {
    if (!padSelCode) return;
    void padTesterTouche(padSelCode, padPlage);
  }

  // ===== Attribution =====
  let listeFollowers = $derived($followers?.liste ?? []);
  let streamer = $derived($broadcaster);
  let registre = $derived($welcomeRegistre);
  let progress = $derived($welcomeAttributionProgress);
  let attributing = $state(false);

  // Le streamer lui-même ne peut pas se suivre sur Twitch, donc il n'est pas
  // dans la liste des followers. On l'ajoute en tête pour qu'il puisse tester
  // ses propres clips de bienvenue.
  let listeAvecStreamer = $derived(
    streamer && streamer.user_id && streamer.login
      ? [
          { login: streamer.login, user_id: streamer.user_id, followed_at: "" },
          ...listeFollowers.filter((f) => f.login !== streamer!.login),
        ]
      : listeFollowers,
  );

  // Recherche followers (filtre la liste par login).
  let searchQuery = $state("");
  let followersFiltres = $derived(
    searchQuery.trim() === ""
      ? listeAvecStreamer
      : listeAvecStreamer.filter((f) =>
          f.login.toLowerCase().includes(searchQuery.trim().toLowerCase()),
        ),
  );

  let selectedLogin = $state<string | null>(null);
  let clipsStreamer = $state<WelcomeClipInfo[]>([]);
  let loadingClips = $state(false);

  let registreMap = $derived(new Map(registre));

  function configPourLogin(login: string): WelcomeViewerConfig | undefined {
    return registreMap.get(login);
  }

  async function onSelectionnerFollower(login: string, userId: string) {
    selectedLogin = login;
    clipsStreamer = [];
    loadingClips = true;
    try {
      clipsStreamer = await tauri.welcomeListerClipsStreamer(userId, 20);
    } catch {
    }
    loadingClips = false;
  }

  async function onAttribuerManuel(clip: WelcomeClipInfo) {
    if (!selectedLogin) return;
    const login = selectedLogin;

    // Résoudre l'URL MP4 signée à l'attribution (pas au déclenchement) pour
    // éviter la latence réseau au 1er message. Si la résolution échoue, on
    // stocke vide → résolution à la volée au déclenchement (fallback).
    let mp4Url = "";
    try {
      mp4Url = await tauri.welcomeResoudreMp4(clip.id);
    } catch {
    }

    const config: WelcomeViewerConfig = {
      actif: true,
      clip_id: clip.id,
      clip_titre: clip.title,
      clip_thumbnail: clip.thumbnail_url,
      clip_mp4_url: mp4Url,
      clip_duree_ms: Math.round(clip.duration * 1000),
      message: get(t)("interactions.bienvenue.messageDefaut", { login }),
      display_name: login,
      avatar: null,
    };
    try {
      await welcomeSauverViewer(login, config);
    } catch {
    }
  }

  // Test manuel : lance un clip côté diffusion (bypass queue) au clic.
  // Track quel clip est en cours de test → le bouton devient "Stop".
  let testingClipId = $state<string | null>(null);

  // Quand current devient null (clip terminé / stoppé), clear testingClipId.
  $effect(() => {
    if (!current) {
      testingClipId = null;
    }
  });

  async function onTesterClip(clip: WelcomeClipInfo) {
    if (testingClipId) return;
    if (!selectedLogin) return;
    testingClipId = clip.id;
    try {
      await welcomeTesterClip(
        clip.id,
        clip.title,
        Math.round(clip.duration * 1000),
        selectedLogin,
      );
    } catch {
      testingClipId = null;
    }
  }

  async function onStopperTest() {
    await welcomeSkip();
    testingClipId = null;
  }

  async function onAttribuerAuto() {
    if (listeAvecStreamer.length === 0) return;
    attributing = true;
    try {
      // Inclut le streamer en tête de liste (il ne peut pas se suivre
      // lui-même sur Twitch, donc on l'ajoute manuellement pour qu'il
      // puisse lui aussi avoir un clip de bienvenue).
      const followersInput = listeAvecStreamer.map((f) => ({
        login: f.login,
        user_id: f.user_id,
        display_name: f.login,
      }));
      await welcomeAttribuerAuto(followersInput);
    } catch {
    }
    attributing = false;
  }

  async function onSupprimer(login: string) {
    try {
      await welcomeSupprimerViewer(login);
    } catch {
    }
  }

  async function onToggleActif(login: string) {
    const config = configPourLogin(login);
    if (!config) return;
    try {
      await welcomeSauverViewer(login, { ...config, actif: !config.actif });
    } catch {
    }
  }

  $effect(() => {
    if (!followers) {
      chargerFollowers();
    }
  });

  // Charge les commandes chat (une fois à l'ouverture de la modale).
  $effect(() => {
    void chargerCommandes();
  });

  // ===== File d'attente =====
  let etat = $derived($welcomeEtat);
  let actif = $derived(etat?.config_globale_actif ?? false);
  let current = $derived(etat?.current ?? null);
  let queue = $derived(etat?.en_attente ?? []);

  // ===== Durée d'affichage globale =====
  // 0 = durée naturelle du clip, > 0 = forcé (en secondes pour le slider).
  let dureeAffichageMs = $derived(etat?.duree_affichage_ms ?? 0);
  let dureeAffichageSec = $derived(dureeAffichageMs / 1000);

  async function onDureeInput(e: Event) {
    const sec = parseFloat((e.target as HTMLInputElement).value);
    await welcomeSetDureeAffichage(Math.round(sec * 1000));
  }

  // ===== Config overlay : mini-canvas drag/resize (onglet "Squelette de position") =====
  // Le canvas de référence est 1920x1080 (défaut diffusion). Le mini-canvas
  // affiche une version réduite où l'utilisateur drag/resize le squelette
  // de l'overlay. Les coordonnées sont converties en pixels canvas réels.
  //
  // Le squelette est un rectangle LIBRE (pas de ratio contraint) : la position
  // et la taille sont partagées par les overlays "clip de bienvenue" et
  // "alertes". Côté diffusion, la hauteur est auto (la carte s'ajuste à son
  // contenu) — la hauteur ici est une référence visuelle pour le squelette.
  let overlay = $derived($positionOverlay);

  // Dimensions canvas de référence (récupérées depuis la scène si dispo,
  // sinon 1920x1080).
  const REF_W = 1920;
  const REF_H = 1080;

  // Valeurs locales (édition fluide pendant le drag, commit au pointerup).
  // Initialisées avec les défauts — le $effect ci-dessous sync depuis
  // l'état Rust dès qu'il arrive.
  let ovX = $state(1480);
  let ovY = $state(580);
  let ovW = $state(320);
  let ovH = $state(352);

  // Sync depuis l'état Rust quand il change (sans écraser pendant le drag).
  let dragging = $state(false);
  $effect(() => {
    if (overlay && !dragging) {
      ovX = overlay.x;
      ovY = overlay.y;
      ovW = overlay.largeur;
      ovH = overlay.hauteur;
    }
  });

  // Dimensions réelles du mini-canvas (mesurées via bind:clientWidth).
  // L'échelle utilise la largeur seule — la hauteur du mini-canvas est libre.
  let miniW = $state(300);

  // Échelle : pixels mini-canvas → pixels canvas réels.
  let scale = $derived(miniW / REF_W);

  async function onOverlayCommit() {
    const cfg: PositionOverlayConfig = {
      x: Math.round(ovX),
      y: Math.round(ovY),
      largeur: Math.round(ovW),
      hauteur: Math.round(ovH),
    };
    try {
      await sauverPositionOverlay(cfg);
    } catch {
    }
  }

  // Presets 6 positions (partagés avec l'ancien bloc alertes).
  function presetOverlay(pos: string) {
    const cw = $sceneStore.canvasW ?? 1920;
    const ch = $sceneStore.canvasH ?? 1080;
    const w = ovW;
    const h = ovH;
    const marge = 40;
    let x = ovX;
    let y = ovY;
    if (pos.includes("gauche")) x = marge;
    else if (pos.includes("droite")) x = cw - w - marge;
    else x = Math.round((cw - w) / 2);
    if (pos.startsWith("haut")) y = marge;
    else y = ch - h - marge;
    ovX = x;
    ovY = y;
    void onOverlayCommit();
  }

  // ===== Drag / resize du squelette overlay =====
  type DragMode = "move" | "resize-se" | "resize-sw" | "resize-ne" | "resize-nw" | null;
  let dragMode = $state<DragMode>(null);
  let dragStart = { px: 0, py: 0, x: 0, y: 0, w: 0, h: 0 };

  function onRectDown(e: PointerEvent) {
    e.stopPropagation();
    e.preventDefault();
    dragMode = "move";
    dragging = true;
    dragStart = { px: e.clientX, py: e.clientY, x: ovX, y: ovY, w: ovW, h: ovH };
    (e.target as HTMLElement).setPointerCapture(e.pointerId);
  }

  function onHandleDown(e: PointerEvent, mode: DragMode) {
    e.stopPropagation();
    e.preventDefault();
    dragMode = mode;
    dragging = true;
    dragStart = { px: e.clientX, py: e.clientY, x: ovX, y: ovY, w: ovW, h: ovH };
    (e.target as HTMLElement).setPointerCapture(e.pointerId);
  }

  function onPointerMove(e: PointerEvent) {
    if (!dragMode) return;
    const dx = (e.clientX - dragStart.px) / scale;
    const dy = (e.clientY - dragStart.py) / scale;

    if (dragMode === "move") {
      ovX = Math.max(0, Math.min(REF_W - ovW, dragStart.x + dx));
      ovY = Math.max(0, Math.min(REF_H - ovH, dragStart.y + dy));
    } else if (dragMode === "resize-se") {
      // Rectangle libre : largeur et hauteur suivent indépendamment la souris.
      ovW = Math.max(80, Math.min(REF_W - ovX, dragStart.w + dx));
      ovH = Math.max(60, Math.min(REF_H - ovY, dragStart.h + dy));
    } else if (dragMode === "resize-sw") {
      const newW = Math.max(80, dragStart.w - dx);
      ovX = Math.max(0, dragStart.x + (dragStart.w - newW));
      ovW = newW;
      ovH = Math.max(60, Math.min(REF_H - ovY, dragStart.h + dy));
    } else if (dragMode === "resize-ne") {
      const newH = Math.max(60, dragStart.h - dy);
      ovY = Math.max(0, dragStart.y + (dragStart.h - newH));
      ovW = Math.max(80, Math.min(REF_W - ovX, dragStart.w + dx));
      ovH = newH;
    } else if (dragMode === "resize-nw") {
      const newW = Math.max(80, dragStart.w - dx);
      const newH = Math.max(60, dragStart.h - dy);
      ovX = Math.max(0, dragStart.x + (dragStart.w - newW));
      ovY = Math.max(0, dragStart.y + (dragStart.h - newH));
      ovW = newW;
      ovH = newH;
    }
  }

  async function onPointerUp() {
    if (!dragMode) return;
    dragMode = null;
    dragging = false;
    await onOverlayCommit();
  }

  // ===== Bandeau premier message =====
  let bEtat = $derived($bandeauEtat);
  let bActif = $derived(bEtat?.actif ?? false);
  let bDureeMs = $derived(bEtat?.duree_ms ?? 6000);
  let bDureeSec = $derived(bDureeMs / 1000);
  let bPosition = $derived(bEtat?.position ?? "bas");
  let bPolice = $derived(bEtat?.police ?? "");

  async function onBandeauDureeInput(e: Event) {
    const sec = parseFloat((e.target as HTMLInputElement).value);
    await bandeauSetDuree(Math.round(sec * 1000));
  }

  async function onBandeauTester() {
    await bandeauTester(get(t)("interactions.bandeau.testPseudo"), get(t)("interactions.bandeau.testMessage"));
  }
</script>

<Modal title={$t("interactions.titre")} onClose={onFermer}>
  <div class="tabs">
    <button class="tab" class:active={tab === "bienvenue"} onclick={() => (tab = "bienvenue")}>
      {$t("interactions.tabBienvenue")}
    </button>
    <button class="tab" class:active={tab === "bandeau"} onclick={() => (tab = "bandeau")}>
      {$t("interactions.tabBandeau")}
    </button>
    <button class="tab" class:active={tab === "alertes"} onclick={() => (tab = "alertes")}>
      {$t("interactions.tabAlertes")}
    </button>
    <button class="tab" class:active={tab === "commandes"} onclick={() => (tab = "commandes")}>
      {$t("interactions.tabCommandes")}
    </button>
    <button
      class="tab"
      class:active={tab === "pad"}
      onclick={() => { tab = "pad"; void chargerPad(); }}
    >
      {$t("interactions.tabPad")}
    </button>
    <button class="tab" class:active={tab === "position"} onclick={() => (tab = "position")}>
      {$t("interactions.tabPosition")}
    </button>
  </div>

    {#if tab === "bienvenue"}
      <div class="tab-body">
        <div class="alertes-layout">
          <!-- Colonne gauche : sections de l'onglet -->
          <div class="alertes-liste">
            <button
              class="alerte-select"
              class:selected={secBienvenue === "general"}
              onclick={() => (secBienvenue = "general")}
            >
              <span class="alerte-nom">{$t("commun.general")}</span>
            </button>
            <button
              class="alerte-select"
              class:selected={secBienvenue === "file"}
              onclick={() => (secBienvenue = "file")}
            >
              <span class="alerte-nom">{$t("interactions.bienvenue.secFile")}</span>
            </button>
            <button
              class="alerte-select"
              class:selected={secBienvenue === "attribution"}
              onclick={() => (secBienvenue = "attribution")}
            >
              <span class="alerte-nom">{$t("interactions.bienvenue.secAttribution")}</span>
            </button>
          </div>

          <!-- Panneau droit : contenu de la section sélectionnée -->
          <div class="alerte-detail">
            {#if secBienvenue === "general"}
              <!-- Config globale + overlay -->
              <div class="bloc">
                <div class="bloc-titre">{$t("commun.general")}</div>
                <label class="toggle">
                  <input
                    type="checkbox"
                    checked={actif}
                    onchange={(e) => welcomeSetConfigGlobaleActif(e.currentTarget.checked)}
                  />
                  <span>{$t("interactions.bienvenue.actifs")}</span>
                </label>

                <div class="duree-row">
                  <span class="field-label">{$t("interactions.bienvenue.dureeAffichage")}</span>
                  <span class="duree-val">
                    {dureeAffichageSec === 0 ? $t("interactions.bienvenue.naturelle") : `${dureeAffichageSec}s`}
                  </span>
                </div>
                <input
                  class="range"
                  type="range"
                  min="0"
                  max="30"
                  step="1"
                  value={dureeAffichageSec}
                  oninput={onDureeInput}
                />
                <p class="hint">{$t("interactions.bienvenue.dureeHint")}</p>
              </div>
            {/if}

            {#if secBienvenue === "file"}
              <!-- File d'attente -->
              <div class="bloc">
                <div class="bloc-titre">{$t("interactions.bienvenue.fileTitre", { n: queue.length })}</div>

                {#if current}
                  <div class="current">
                    <div class="current-label">{$t("interactions.bienvenue.enCours")}</div>
                    <div class="item">
                      {#if current.avatar}
                        <img src={current.avatar} alt="" class="avatar" />
                      {/if}
                      <div class="item-info">
                        <div class="item-pseudo">{current.display_name}</div>
                        <div class="item-clip">{current.clip_titre}</div>
                      </div>
                      <div class="item-actions">
                        <button title={$t("interactions.bienvenue.skipTitre")} onclick={() => welcomeSkip()}>⏭</button>
                        <button title={$t("interactions.bienvenue.stopTout")} onclick={() => welcomeStop()}>⏹</button>
                      </div>
                    </div>
                  </div>
                {/if}

                <div class="queue-actions">
                  <button onclick={() => welcomeVider()} disabled={queue.length === 0}>
                    {$t("commun.vider")}
                  </button>
                  <button onclick={() => welcomeResetSession()}>
                    {$t("commun.resetSession")}
                  </button>
                </div>

                {#if queue.length === 0}
                  <p class="empty">{$t("interactions.bienvenue.aucunClip")}</p>
                {:else}
                  <ul class="queue-list">
                    {#each queue as item, i (item.id)}
                      <li class="item">
                        {#if item.avatar}
                          <img src={item.avatar} alt="" class="avatar" />
                        {/if}
                        <div class="item-info">
                          <div class="item-pseudo">{item.display_name}</div>
                          <div class="item-clip">{item.clip_titre}</div>
                        </div>
                        <div class="item-actions">
                          <button title={get(t)("interactions.bienvenue.remonter")} onclick={() => welcomeRemonter(item.id)} disabled={i === 0}>↑</button>
                          <button title={get(t)("interactions.bienvenue.descendre")} onclick={() => welcomeDescendre(item.id)} disabled={i === queue.length - 1}>↓</button>
                          <button title={get(t)("commun.retirer")} onclick={() => welcomeRetirer(item.id)}>✕</button>
                        </div>
                      </li>
                    {/each}
                  </ul>
                {/if}
              </div>
            {/if}

            {#if secBienvenue === "attribution"}
              <!-- Attribution followers → clips -->
              <div class="bloc">
                <div class="bloc-titre">{$t("interactions.bienvenue.attributionTitre")}</div>
          <div class="actions-bar">
            <button
              class="action"
              onclick={onAttribuerAuto}
              disabled={attributing || listeAvecStreamer.length === 0}
            >
              {attributing ? $t("interactions.bienvenue.attributionEnCours") : $t("interactions.bienvenue.attributionAuto")}
            </button>
            {#if progress}
              <span class="progress">
                {$t("interactions.bienvenue.progressResult", { succes: progress.succes, echecs: progress.echecs, login: progress.login })}
              </span>
            {/if}
          </div>

          <div class="attribution-body">
            <div class="followers-list">
              <h3>{$t("interactions.bienvenue.followersTitre", { n: listeAvecStreamer.length })}</h3>
              <input
                class="search-input"
                type="text"
                placeholder={$t("interactions.bienvenue.rechercherFollower")}
                bind:value={searchQuery}
              />
              {#if listeAvecStreamer.length === 0}
                <p class="empty">{$t("interactions.bienvenue.aucunFollower")}</p>
              {:else if followersFiltres.length === 0}
                <p class="empty">{$t("interactions.bienvenue.aucunFollowerMatch", { q: searchQuery })}</p>
              {:else}
                <ul>
                  {#each followersFiltres as f (f.login)}
                    {@const config = configPourLogin(f.login)}
                    <li>
                      <button
                        class="follower-row"
                        class:selected={selectedLogin === f.login}
                        onclick={() => onSelectionnerFollower(f.login, f.user_id)}
                      >
                        <span class="login">{f.login}</span>
                        {#if config}
                          <span class="badge" class:actif={config.actif} class:inactif={!config.actif}>
                            {config.actif ? "✓" : "○"}
                          </span>
                        {/if}
                      </button>
                      {#if config}
                        <button class="mini-btn" title={config.actif ? get(t)("commun.desactiver") : get(t)("commun.activer")} onclick={() => onToggleActif(f.login)}>
                          {config.actif ? "⏸" : "▶"}
                        </button>
                        <button class="mini-btn" title={get(t)("commun.supprimer")} onclick={() => onSupprimer(f.login)}>✕</button>
                      {/if}
                    </li>
                  {/each}
                </ul>
              {/if}
            </div>

            {#if selectedLogin}
              <div class="clip-picker">
                <h3>{$t("interactions.bienvenue.clipDe", { login: selectedLogin })}</h3>
                {#if loadingClips}
                  <p class="empty">{$t("interactions.bienvenue.chargementClips")}</p>
                {:else if clipsStreamer.length === 0}
                  <p class="empty">{$t("interactions.bienvenue.aucunClipDispo")}</p>
                {:else}
                  <ul class="clips-list">
                    {#each clipsStreamer as clip (clip.id)}
                      {@const config = configPourLogin(selectedLogin)}
                      <li>
                        <div class="clip-row" class:attribue={config?.clip_id === clip.id}>
                          <img src={clip.thumbnail_url} alt="" class="thumb" />
                          <div class="clip-info">
                            <div class="clip-title">{clip.title}</div>
                            <div class="clip-meta">{Math.round(clip.duration)}s · {clip.created_at.slice(0, 10)}</div>
                          </div>
                        </div>
                        <div class="clip-actions">
                          <button
                            class="mini-btn"
                            title={get(t)("interactions.bienvenue.attribuerTitle")}
                            onclick={() => onAttribuerManuel(clip)}
                          >
                            {get(t)("interactions.bienvenue.attribuer")}
                          </button>
                          {#if testingClipId === clip.id}
                            <button
                              class="test-btn stop"
                              title={get(t)("interactions.bienvenue.arreterClip")}
                              onclick={onStopperTest}
                            >
                              ⏹ {get(t)("widgetSpeedrun.stop")}
                            </button>
                          {:else}
                            <button
                              class="test-btn"
                              title={get(t)("interactions.bienvenue.testerTitle")}
                              onclick={() => onTesterClip(clip)}
                              disabled={!!testingClipId}
                            >
                              {get(t)("commun.tester")}
                            </button>
                          {/if}
                        </div>
                      </li>
                    {/each}
                  </ul>
                {/if}
              </div>
            {/if}
              </div>
            </div>
            {/if}
            </div>
          </div>
        </div>
      {/if}

    {#if tab === "bandeau"}
      <div class="tab-body">
        <div class="alertes-layout">
          <!-- Colonne gauche : sections de l'onglet -->
          <div class="alertes-liste">
            <button
              class="alerte-select"
              class:selected={secBandeau === "general"}
              onclick={() => (secBandeau = "general")}
            >
              <span class="alerte-nom">{$t("commun.general")}</span>
            </button>
            <button
              class="alerte-select"
              class:selected={secBandeau === "duree"}
              onclick={() => (secBandeau = "duree")}
            >
              <span class="alerte-nom">{$t("commun.duree")}</span>
            </button>
            <button
              class="alerte-select"
              class:selected={secBandeau === "position"}
              onclick={() => (secBandeau = "position")}
            >
              <span class="alerte-nom">{$t("interactions.bandeau.secPosition")}</span>
            </button>
            <button
              class="alerte-select"
              class:selected={secBandeau === "test"}
              onclick={() => (secBandeau = "test")}
            >
              <span class="alerte-nom">{$t("interactions.bandeau.secTest")}</span>
            </button>
            <button
              class="alerte-select"
              class:selected={secBandeau === "apparence"}
              onclick={() => (secBandeau = "apparence")}
            >
              <span class="alerte-nom">{$t("interactions.bandeau.secApparence")}</span>
            </button>
          </div>

          <!-- Panneau droit : contenu de la section -->
          <div class="alerte-detail">
            {#if secBandeau === "general"}
              <div class="bloc">
                <div class="bloc-titre">{$t("commun.general")}</div>
                <label class="toggle">
                  <input
                    type="checkbox"
                    checked={bActif}
                    onchange={(e) => bandeauSetActif(e.currentTarget.checked)}
                  />
                  <span>{$t("interactions.bandeau.actif")}</span>
                </label>
                <p class="hint">
                  {$t("interactions.bandeau.actifHint", { pos: bPosition === "haut" ? $t("interactions.bandeau.posHaut") : $t("interactions.bandeau.posBas") })}
                </p>
              </div>
            {:else if secBandeau === "duree"}
              <div class="bloc">
                <div class="bloc-titre">{$t("interactions.bandeau.dureeAffichage")}</div>
                <div class="duree-row">
                  <span class="field-label">{$t("commun.duree")}</span>
                  <span class="duree-val">{bDureeSec}s</span>
                </div>
                <input
                  class="range"
                  type="range"
                  min="2"
                  max="20"
                  step="1"
                  value={bDureeSec}
                  oninput={onBandeauDureeInput}
                />
                <p class="hint">{$t("interactions.bandeau.dureeHint")}</p>
              </div>
            {:else if secBandeau === "position"}
              <div class="bloc">
                <div class="bloc-titre">{$t("interactions.bandeau.position")}</div>
                <div class="position-row">
                  <button
                    class="pos-btn"
                    class:active={bPosition === "bas"}
                    onclick={() => bandeauSetPosition("bas")}
                  >
                    ▾ {$t("interactions.bandeau.bas")}
                  </button>
                  <button
                    class="pos-btn"
                    class:active={bPosition === "haut"}
                    onclick={() => bandeauSetPosition("haut")}
                  >
                    ▴ {$t("interactions.bandeau.haut")}
                  </button>
                </div>
              </div>
            {:else if secBandeau === "test"}
              <div class="bloc">
                <div class="bloc-titre">{$t("interactions.bandeau.secTest")}</div>
                <div class="queue-actions">
                  <button onclick={onBandeauTester} disabled={!bActif}>
                    {$t("interactions.bandeau.tester")}
                  </button>
                  <button onclick={() => bandeauResetSession()}>
                    {$t("commun.resetSession")}
                  </button>
                </div>
                <p class="hint">
                  {$t("interactions.bandeau.testHint")}
                </p>
              </div>
            {:else}
              <div class="bloc">
                <div class="bloc-titre">{$t("interactions.bandeau.apparenceTitre")}</div>
                <p class="hint">
                  {$t("interactions.bandeau.apparenceHint")}
                </p>
              </div>
              <div class="bloc">
                <div class="bloc-titre">{$t("commun.police")}</div>
                <!-- Sélection persistée au change (set_config → disque + WS). -->
                <select
                  class="police-select"
                  value={bPolice}
                  onchange={(e) => bandeauSetPolice(e.currentTarget.value)}
                >
                  <option value="">{$t("interactions.bandeau.policeSysteme")}</option>
                  {#each POLICES_TITRE as p (p.id)}
                    <option value={p.id} style="font-family: '{p.id}', sans-serif;">{p.nom}</option>
                  {/each}
                </select>
              </div>
            {/if}
          </div>
        </div>
      </div>
    {/if}

    {#if tab === "alertes"}
      <div class="tab-body">
        <p class="hint">
          {$t("interactions.alertes.hintIntro")}
        </p>

        <div class="alertes-layout">
          <!-- Colonne gauche : liste des types (nom + ON/OFF) -->
          <div class="alertes-liste">
            {#each TYPES_ALERTE as ty (ty)}
              {@const c = cfgType(ty)}
              <div class="alerte-item" class:selected={typeActif === ty}>
                <button
                  class="alerte-select"
                  class:selected={typeActif === ty}
                  onclick={() => (typeActif = ty)}
                >
                  <span class="alerte-nom">{NOMS_ALERTE[ty]}</span>
                </button>
                <button
                  class="alerte-toggle"
                  class:on={c.actif}
                  onclick={() => toggleAlerteActif(ty)}
                  title={c.actif ? get(t)("interactions.alertes.desactiverType") : get(t)("interactions.alertes.activerType")}
                >{c.actif ? "ON" : "OFF"}</button>
              </div>
            {/each}
          </div>

          <!-- Panneau droit : config du type sélectionné -->
          <div class="alerte-detail">
            <div class="bloc">
              <div class="bloc-titre">{$t("commun.media")}</div>
              <div class="media-modes">
                <button
                  class="mode-btn"
                  class:active={modeMedia(typeActif) === "aucun"}
                  onclick={() => majAlerte(typeActif, { media: undefined, media_kind: undefined, son: undefined })}
                >{$t("commun.aucun")}</button>
                <button
                  class="mode-btn"
                  class:active={modeMedia(typeActif) === "image"}
                  onclick={() => importerMediaAlerte(typeActif, "image")}
                >{$t("commun.image")}</button>
                <button
                  class="mode-btn"
                  class:active={modeMedia(typeActif) === "video"}
                  onclick={() => importerMediaAlerte(typeActif, "video")}
                >{$t("commun.video")}</button>
              </div>
              {#if cfgSel.media}
                <div class="alerte-son">
                  <span class="alerte-son-nom">{cfgSel.media.split("/").pop()}</span>
                  <button
                    class="alerte-toggle"
                    onclick={() => majAlerte(typeActif, { media: undefined, media_kind: undefined })}
                    title={$t("commun.retirerMedia")}
                  >✕</button>
                </div>
              {/if}
              {#if modeMedia(typeActif) === "image"}
                <div class="alerte-son">
                  {#if cfgSel.son}
                    <span class="alerte-son-nom">{cfgSel.son.split("/").pop()}</span>
                    <button
                      class="alerte-toggle"
                      onclick={() => majAlerte(typeActif, { son: undefined })}
                      title={$t("commun.retirerSon")}
                    >✕</button>
                  {:else}
                    <button class="alerte-test" onclick={() => importerSonAlerte(typeActif)}>
                      {$t("commun.importerSon")}
                    </button>
                  {/if}
                </div>
              {/if}
              <p class="hint">
                {$t("interactions.hintMedia")}
              </p>
            </div>

            <div class="bloc">
              <div class="bloc-titre">{$t("commun.texte")}</div>
              <label class="alerte-field">
                <span class="field-label">{$t("commun.texte")}</span>
                <input
                  value={cfgSel.texte_template}
                  spellcheck="false"
                  onchange={(e) => majAlerte(typeActif, { texte_template: (e.target as HTMLInputElement).value })}
                />
              </label>
              <p class="hint">
                {$t("interactions.alertes.variablesHint", { vars: "{pseudo} {nbViewers} {nbBits} {niveauSub} {nbMoisCumul} {destinataire}" })}
              </p>
              <p class="hint">
                {$t("interactions.hintCadre")}
              </p>
              <label class="alerte-field">
                <span class="field-label">{$t("commun.taille")} {cfgSel.taille_px}px</span>
                <input
                  class="range"
                  type="range"
                  min="16"
                  max="96"
                  step="2"
                  value={cfgSel.taille_px}
                  oninput={(e) => majAlerte(typeActif, { taille_px: +(e.target as HTMLInputElement).value })}
                />
              </label>
            </div>

            <div class="bloc">
              <div class="bloc-titre">{$t("commun.comportement")}</div>
              <div class="alerte-2col">
                <label class="alerte-field">
                  <span class="field-label">{$t("commun.dureeS")}</span>
                  <input
                    type="number"
                    min="1"
                    max="60"
                    value={Math.round(cfgSel.duree_ms / 1000)}
                    onchange={(e) => majAlerte(typeActif, { duree_ms: Math.max(1, +(e.target as HTMLInputElement).value) * 1000 })}
                  />
                </label>
                <label class="alerte-field">
                  <span class="field-label">{$t("interactions.cooldownGlobal")}</span>
                  <input
                    type="number"
                    min="0"
                    max="600"
                    value={cfgSel.cooldown_global_s}
                    onchange={(e) => majAlerte(typeActif, { cooldown_global_s: Math.max(0, +(e.target as HTMLInputElement).value) })}
                  />
                </label>
              </div>
              <label class="alerte-field">
                <span class="field-label">{$t("interactions.cooldownViewer")}</span>
                <input
                  type="number"
                  min="0"
                  max="600"
                  value={cfgSel.cooldown_viewer_s}
                  onchange={(e) => majAlerte(typeActif, { cooldown_viewer_s: Math.max(0, +(e.target as HTMLInputElement).value) })}
                />
              </label>
            </div>

            <div class="bloc">
              <div class="bloc-titre">{$t("commun.test")}</div>
              <button class="alerte-test" onclick={() => testerAlerte(typeActif)} disabled={!cfgSel.actif}>
                {$t("interactions.alertes.testerBouton")}
              </button>
              <p class="hint">
                {$t("interactions.testHint")}
              </p>
            </div>
          </div>
        </div>
      </div>
    {/if}

    {#if tab === "commandes"}
      <div class="tab-body">
        <p class="hint">
          {$t("interactions.commandes.hintIntro")}
        </p>

        <div class="alertes-layout">
          <!-- Colonne gauche : liste des commandes + ajout -->
          <div class="alertes-liste">
            {#each $commandesStore as c (c.id)}
              <div class="alerte-item" class:selected={cmdSelId === c.id}>
                <button
                  class="alerte-select"
                  class:selected={cmdSelId === c.id}
                  onclick={() => (cmdSelId = c.id)}
                >
                  <span class="alerte-nom">!{c.commande}</span>
                </button>
                <button
                  class="alerte-toggle"
                  class:on={c.actif}
                  onclick={() => majCommande({ actif: !c.actif })}
                  title={c.actif ? get(t)("interactions.commandes.desactiver") : get(t)("interactions.commandes.activer")}
                >{c.actif ? "ON" : "OFF"}</button>
              </div>
            {/each}
            <button class="alerte-test cmd-ajouter" onclick={onAjouterCommande}>
              + {$t("commun.ajouter")}
            </button>
          </div>

          <!-- Panneau droit : config de la commande sélectionnée -->
          <div class="alerte-detail">
            {#if cmdSel}
              <div class="bloc">
                <div class="bloc-titre">{$t("interactions.commandes.titreBloc")}</div>
                <label class="alerte-field">
                  <span class="field-label">{$t("interactions.commandes.label")}</span>
                  <input
                    value={cmdSel.commande}
                    spellcheck="false"
                    placeholder="hype"
                    onchange={(e) => {
                      const v = (e.target as HTMLInputElement).value.trim().replace(/^!+/, "");
                      if (v === "") {
                        // Nom vide refusé côté Rust → restaurer l'ancien nom.
                        (e.target as HTMLInputElement).value = cmdSel.commande;
                        return;
                      }
                      majCommande({ commande: v });
                    }}
                  />
                </label>
                <p class="hint">
                  {$t("interactions.commandes.hintCommande", { cmd: "!" + (cmdSel.commande || "…") })}
                </p>
              </div>

              <div class="bloc">
                <div class="bloc-titre">{$t("commun.media")}</div>
                <div class="media-modes">
                  <button
                    class="mode-btn"
                    class:active={modeMediaCommande(cmdSel) === "aucun"}
                    onclick={() => majCommande({ media: undefined, media_kind: undefined, son: undefined })}
                  >{$t("commun.aucun")}</button>
                  <button
                    class="mode-btn"
                    class:active={modeMediaCommande(cmdSel) === "image"}
                    onclick={() => importerMediaCommande("image")}
                  >{$t("commun.image")}</button>
                  <button
                    class="mode-btn"
                    class:active={modeMediaCommande(cmdSel) === "video"}
                    onclick={() => importerMediaCommande("video")}
                  >{$t("commun.video")}</button>
                </div>
                {#if cmdSel.media}
                  <div class="alerte-son">
                    <span class="alerte-son-nom">{cmdSel.media.split("/").pop()}</span>
                    <button
                      class="alerte-toggle"
                      onclick={() => majCommande({ media: undefined, media_kind: undefined })}
                      title={$t("commun.retirerMedia")}
                    >✕</button>
                  </div>
                {/if}
                {#if modeMediaCommande(cmdSel) === "image"}
                  <div class="alerte-son">
                    {#if cmdSel.son}
                      <span class="alerte-son-nom">{cmdSel.son.split("/").pop()}</span>
                      <button
                        class="alerte-toggle"
                        onclick={() => majCommande({ son: undefined })}
                        title={$t("commun.retirerSon")}
                      >✕</button>
                    {:else}
                      <button class="alerte-test" onclick={importerSonCommande}>
                        {$t("commun.importerSon")}
                      </button>
                    {/if}
                  </div>
                {/if}
                <p class="hint">
                  {$t("interactions.hintMedia")}
                </p>
              </div>

              <div class="bloc">
                <div class="bloc-titre">{$t("commun.texte")}</div>
                <label class="alerte-field">
                  <span class="field-label">{$t("commun.texte")}</span>
                  <input
                    value={cmdSel.texte_template}
                    spellcheck="false"
                    onchange={(e) => majCommande({ texte_template: (e.target as HTMLInputElement).value })}
                  />
                </label>
                <p class="hint">
                  {$t("interactions.commandes.variablesHint")}
                </p>
                <p class="hint">
                  {$t("interactions.hintCadre")}
                </p>
                <label class="alerte-field">
                  <span class="field-label">{$t("commun.taille")} {cmdSel.taille_px}px</span>
                  <input
                    class="range"
                    type="range"
                    min="16"
                    max="96"
                    step="2"
                    value={cmdSel.taille_px}
                    oninput={(e) => majCommande({ taille_px: +(e.target as HTMLInputElement).value })}
                  />
                </label>
              </div>

              <div class="bloc">
                <div class="bloc-titre">{$t("commun.comportement")}</div>
                <div class="alerte-2col">
                  <label class="alerte-field">
                    <span class="field-label">{$t("commun.dureeS")}</span>
                    <input
                      type="number"
                      min="1"
                      max="60"
                      value={Math.round(cmdSel.duree_ms / 1000)}
                      onchange={(e) => majCommande({ duree_ms: Math.max(1, +(e.target as HTMLInputElement).value) * 1000 })}
                    />
                  </label>
                  <label class="alerte-field">
                    <span class="field-label">{$t("interactions.cooldownGlobal")}</span>
                    <input
                      type="number"
                      min="0"
                      max="600"
                      value={cmdSel.cooldown_global_s}
                      onchange={(e) => majCommande({ cooldown_global_s: Math.max(0, +(e.target as HTMLInputElement).value) })}
                    />
                  </label>
                </div>
                <label class="alerte-field">
                  <span class="field-label">{$t("interactions.cooldownViewer")}</span>
                  <input
                    type="number"
                    min="0"
                    max="600"
                    value={cmdSel.cooldown_viewer_s}
                    onchange={(e) => majCommande({ cooldown_viewer_s: Math.max(0, +(e.target as HTMLInputElement).value) })}
                  />
                </label>
              </div>

              <div class="bloc">
                <div class="bloc-titre">{$t("interactions.testSuppression")}</div>
                <div class="queue-actions">
                  <button class="alerte-test" onclick={() => testerCommande(cmdSel.id)} disabled={!cmdSel.actif}>
                    {$t("interactions.commandes.tester")}
                  </button>
                  <button
                    class="alerte-toggle"
                    onclick={() => { void supprimerCommande(cmdSel.id); }}
                    title={$t("interactions.commandes.supprimerTitle")}
                  >{$t("commun.supprimer")}</button>
                </div>
                <p class="hint">
                  {$t("interactions.testHint")}
                </p>
              </div>
            {:else}
              <div class="bloc">
                <p class="hint">{$t("interactions.commandes.aucuneSelection")}</p>
              </div>
          {/if}
          </div>
        </div>
      </div>
    {/if}

    {#if tab === "pad"}
      <div class="tab-body">
        <p class="hint">
          {$t("interactions.pad.hintIntro")}
        </p>

        {#if padErr}
          <div class="pad-erreur">⚠ {padErr}</div>
        {/if}

        <div class="alertes-layout">
          <!-- Colonne gauche : activation + plages + grille 4×4 -->
          <div class="alertes-liste">
            <div class="bloc">
              <div class="bloc-titre">{$t("interactions.pad.activation")}</div>
              <button
                class="alerte-toggle"
                class:on={padActif}
                onclick={() => void padSetActif(!padActif)}
                title={padActif ? $t("interactions.pad.desactiver") : $t("interactions.pad.activer")}
              >{padActif ? "ON" : "OFF"}</button>
              <p class="hint">
                {padActif
                  ? $t("interactions.pad.hintActif")
                  : $t("interactions.pad.hintInactif")}
              </p>
            </div>

            <div class="bloc">
              <div class="bloc-titre">{$t("interactions.pad.plages")}</div>
              <div class="pad-plages">
                {#each Array(NB_PLAGES) as _, i}
                  <button
                    class="pad-plage-btn"
                    class:active={padPlage === i}
                    onclick={() => void padSetPlage(i)}
                  >{get(t)("interactions.pad.plage", { n: i + 1 })}</button>
                {/each}
              </div>
              <p class="hint">
                {$t("interactions.pad.plageCourante", { n: padPlage + 1, nb: NB_PLAGES })}
              </p>
            </div>

            <div class="bloc">
              <div class="bloc-titre">{$t("interactions.pad.touches", { n: padPlage + 1 })}</div>
              <div class="pad-grille">
                {#each DISPOSITION_PAD as ligne}
                  {#each ligne as code}
                    {@const reservee = TOUCHES_RESERVEES.includes(code)}
                    {@const touche = pad?.touches[`${code}_${padPlage}`]}
                    {@const mode = modeMediaPad(touche ?? null)}
                    <button
                      class="pad-touche"
                      class:reservee
                      class:selected={padSelCode === code}
                      class:audio={mode === "audio"}
                      class:image={mode === "image"}
                      class:video={mode === "video"}
                      onclick={() => (padSelCode = reservee ? null : code)}
                      title={reservee ? get(t)("interactions.pad.reservee" + code) : code}
                    >
                      <span class="pad-touche-nom">{NOMS_AFFICHAGE[code] ?? code}</span>
                      {#if reservee}
                        <span class="pad-touche-badge reservee">{get(t)("interactions.pad.reservee")}</span>
                      {:else if mode !== "aucun"}
                        <span class="pad-touche-badge">{mode}</span>
                      {/if}
                    </button>
                  {/each}
                {/each}
              </div>
            </div>
          </div>

          <!-- Panneau droit : config de la touche sélectionnée -->
          <div class="alerte-detail">
            {#if padSelCode && padSel}
              <div class="bloc">
                <div class="bloc-titre">{$t("interactions.pad.toucheTitre", { touche: NOMS_AFFICHAGE[padSelCode] ?? padSelCode, plage: padPlage + 1 })}</div>
                <p class="hint">
                  {$t("interactions.pad.hintTouche")}
                </p>
              </div>

              <div class="bloc">
                <div class="bloc-titre">{$t("commun.media")}</div>
                <div class="media-modes">
                  <button
                    class="mode-btn"
                    class:active={modeMediaPad(padSel) === "aucun"}
                    onclick={() => majPad({ media: undefined, media_kind: undefined, media_type: undefined, son: undefined })}
                  >{$t("commun.aucun")}</button>
                  <button
                    class="mode-btn"
                    class:active={modeMediaPad(padSel) === "audio"}
                    onclick={onImporterSonPad}
                  >{$t("commun.audio")}</button>
                  <button
                    class="mode-btn"
                    class:active={modeMediaPad(padSel) === "image"}
                    onclick={() => onImporterMediaPad("image")}
                  >{$t("commun.image")}</button>
                  <button
                    class="mode-btn"
                    class:active={modeMediaPad(padSel) === "video"}
                    onclick={() => onImporterMediaPad("video")}
                  >{$t("commun.video")}</button>
                </div>

                {#if padSel.media}
                  <div class="alerte-son">
                    <span class="alerte-son-nom">{padSel.media.split("/").pop()}</span>
                    <button
                      class="alerte-toggle"
                      onclick={onRetirerMediaPad}
                      title={$t("commun.retirerMedia")}
                    >✕</button>
                  </div>
                {/if}

                {#if modeMediaPad(padSel) === "image"}
                  <div class="alerte-son">
                    {#if padSel.son}
                      <span class="alerte-son-nom">{padSel.son.split("/").pop()}</span>
                      <button
                        class="alerte-toggle"
                        onclick={onRetirerSonPad}
                        title={$t("commun.retirerSon")}
                      >✕</button>
                    {:else}
                      <button class="alerte-test" onclick={onImporterSonPad}>
                        {$t("commun.importerSon")}
                      </button>
                    {/if}
                  </div>
                {/if}

                <p class="hint">
                  {$t("interactions.pad.hintMedia")}
                </p>
              </div>

              <div class="bloc">
                <div class="bloc-titre">{$t("commun.comportement")}</div>
                <div class="alerte-2col">
                  <label class="alerte-field">
                    <span class="field-label">{$t("commun.dureeS")}</span>
                    <input
                      type="number"
                      min="1"
                      max="60"
                      value={Math.round(padSel.duree_ms / 1000)}
                      onchange={(e) => majPad({ duree_ms: Math.max(1, +(e.target as HTMLInputElement).value) * 1000 })}
                    />
                  </label>
                  <label class="alerte-field">
                    <span class="field-label">{$t("interactions.pad.volume", { n: Math.round(padSel.volume * 100) })}</span>
                    <input
                      class="range"
                      type="range"
                      min="0"
                      max="100"
                      step="5"
                      value={Math.round(padSel.volume * 100)}
                      oninput={(e) => majPad({ volume: +(e.target as HTMLInputElement).value / 100 })}
                    />
                  </label>
                </div>
                {#if modeMediaPad(padSel) === "image" || modeMediaPad(padSel) === "video"}
                  <div class="alerte-2col">
                    <label class="alerte-field">
                      <span class="field-label">Bounce</span>
                      <input
                        type="checkbox"
                        checked={padSel.bounce}
                        onchange={(e) => majPad({ bounce: (e.target as HTMLInputElement).checked })}
                      />
                    </label>
                    <label class="alerte-field">
                      <span class="field-label">Zoom</span>
                      <input
                        type="checkbox"
                        checked={padSel.zoom}
                        onchange={(e) => majPad({ zoom: (e.target as HTMLInputElement).checked })}
                      />
                    </label>
                  </div>
                {/if}
              </div>

              <div class="bloc">
                <div class="bloc-titre">{$t("interactions.testSuppression")}</div>
                <div class="queue-actions">
                  <button class="alerte-test" onclick={onTesterTouchePad} disabled={!padActif && modeMediaPad(padSel) === "aucun"}>
                    {$t("interactions.pad.tester")}
                  </button>
                  <button
                    class="alerte-toggle"
                    onclick={onSupprimerTouchePad}
                    title={$t("interactions.pad.reinitialiserTitle")}
                  >{$t("commun.reinitialiser")}</button>
                </div>
                <p class="hint">
                  {$t("interactions.pad.testHint")}
                </p>
              </div>
            {:else}
              <div class="bloc">
                <p class="hint">
                  {$t("interactions.pad.hintSelection")}
                </p>
              </div>
            {/if}
          </div>
        </div>
      </div>
    {/if}

    {#if tab === "position"}
      <div class="tab-body">
        <p class="hint">
          {$t("interactions.position.hintIntro")}
        </p>

        <div class="bloc">
          <div class="bloc-titre">{$t("interactions.tabPosition")}</div>
          <div class="alerte-presets">
            {#each Object.entries(PRESETS_OVERLAY) as [pos, label] (pos)}
              <button class="alerte-test" title={get(t)("interactions.position.pos." + pos)} onclick={() => presetOverlay(pos)}>{label}</button>
            {/each}
          </div>
          <div
            class="mini-canvas"
            bind:clientWidth={miniW}
            onpointermove={onPointerMove}
            onpointerup={onPointerUp}
            role="presentation"
          >
            <div
              class="overlay-skeleton"
              style="left: {ovX * scale}px; top: {ovY * scale}px; width: {ovW * scale}px; height: {ovH * scale}px;"
              onpointerdown={onRectDown}
              role="presentation"
            >
              <span class="skeleton-label">{$t("interactions.position.overlay")}</span>
              <div class="sk-handle nw" onpointerdown={(e) => onHandleDown(e, "resize-nw")} role="presentation"></div>
              <div class="sk-handle ne" onpointerdown={(e) => onHandleDown(e, "resize-ne")} role="presentation"></div>
              <div class="sk-handle sw" onpointerdown={(e) => onHandleDown(e, "resize-sw")} role="presentation"></div>
              <div class="sk-handle se" onpointerdown={(e) => onHandleDown(e, "resize-se")} role="presentation"></div>
            </div>
          </div>
          <p class="hint">
            {$t("interactions.position.hintGlisser", { w: REF_W, h: REF_H })}
          </p>
          <div class="alerte-4col">
            <label class="alerte-field">
              <span class="field-label">X</span>
              <input
                type="number"
                value={Math.round(ovX)}
                onchange={(e) => { ovX = +(e.target as HTMLInputElement).value; void onOverlayCommit(); }}
              />
            </label>
            <label class="alerte-field">
              <span class="field-label">Y</span>
              <input
                type="number"
                value={Math.round(ovY)}
                onchange={(e) => { ovY = +(e.target as HTMLInputElement).value; void onOverlayCommit(); }}
              />
            </label>
            <label class="alerte-field">
              <span class="field-label">{$t("commun.largeur")}</span>
              <input
                type="number"
                min="80"
                max="1920"
                value={Math.round(ovW)}
                onchange={(e) => { ovW = Math.max(80, +(e.target as HTMLInputElement).value); void onOverlayCommit(); }}
              />
            </label>
            <label class="alerte-field">
              <span class="field-label">{$t("interactions.position.hauteurRef")}</span>
              <input
                type="number"
                min="60"
                max="1080"
                value={Math.round(ovH)}
                onchange={(e) => { ovH = Math.max(60, +(e.target as HTMLInputElement).value); void onOverlayCommit(); }}
              />
            </label>
          </div>
        </div>
      </div>
    {/if}
</Modal>

<style>
  /* ===== Onglet Alertes (master-detail : liste gauche + détail droite) ===== */
  .alertes-layout {
    display: grid;
    grid-template-columns: 170px 1fr;
    gap: 0.5rem;
    align-items: start;
  }
  .alertes-liste {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }
  .alerte-item {
    display: flex;
    align-items: center;
    gap: 0.25rem;
  }
  .alerte-select {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 0.45rem;
    min-width: 0;
    background: var(--btn-surface);
    box-shadow: var(--btn-inset);
    color: var(--texte);
    border: 1px solid var(--bordure-ui);
    border-radius: var(--rayon-petit);
    padding: 0.3rem 0.45rem;
    font: inherit;
    font-size: 0.85rem;
    cursor: pointer;
    text-align: left;
    transition: background 0.15s ease, box-shadow 0.15s ease, border-color 0.15s ease;
  }
  .alerte-select:hover {
    background: var(--btn-surface-hover);
    box-shadow: var(--btn-inset-hover);
    border-color: var(--accent-ui);
  }
  .alerte-select.selected {
    background:
      linear-gradient(
        180deg,
        color-mix(in srgb, var(--accent-ui) 25%, var(--fond-controle)) 0%,
        color-mix(in srgb, var(--accent-ui) 12%, var(--fond-controle)) 100%
      );
    color: var(--texte);
    border-color: var(--accent-ui);
  }
  .alerte-detail {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    min-width: 0;
  }
  .cmd-ajouter {
    margin-top: 0.25rem;
  }
  /* Segmented control mode média (Aucun | Image | Vidéo) */
  .media-modes {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 0.25rem;
    margin-bottom: 0.4rem;
  }
  .mode-btn {
    background: var(--btn-surface);
    box-shadow: var(--btn-inset);
    color: var(--texte);
    border: 1px solid var(--bordure-ui);
    border-radius: var(--rayon-petit);
    padding: 0.2rem 0;
    font: inherit;
    font-size: 0.75rem;
    cursor: pointer;
    text-align: center;
    transition: background 0.15s ease, box-shadow 0.15s ease, border-color 0.15s ease;
  }
  .mode-btn:hover {
    background: var(--btn-surface-hover);
    box-shadow: var(--btn-inset-hover);
    border-color: var(--accent-ui);
  }
  .mode-btn.active {
    background:
      linear-gradient(
        180deg,
        color-mix(in srgb, var(--accent-ui) 25%, var(--fond-controle)) 0%,
        color-mix(in srgb, var(--accent-ui) 12%, var(--fond-controle)) 100%
      );
    color: var(--texte);
    border-color: var(--accent-ui);
  }

  /* Pad numérique : plages + grille 4×4 */
  .pad-plages {
    display: flex;
    gap: 0.25rem;
    margin-bottom: 0.4rem;
  }
  .pad-plage-btn {
    flex: 1;
    background: var(--btn-surface);
    box-shadow: var(--btn-inset);
    color: var(--texte);
    border: 1px solid var(--bordure-ui);
    border-radius: var(--rayon-petit);
    padding: 0.3rem 0;
    font: inherit;
    cursor: pointer;
    transition: background 0.15s ease, box-shadow 0.15s ease, border-color 0.15s ease;
  }
  .pad-plage-btn:hover {
    background: var(--btn-surface-hover);
    box-shadow: var(--btn-inset-hover);
    border-color: var(--accent-ui);
  }
  .pad-plage-btn.active {
    background:
      linear-gradient(
        180deg,
        color-mix(in srgb, var(--accent-ui) 25%, var(--fond-controle)) 0%,
        color-mix(in srgb, var(--accent-ui) 12%, var(--fond-controle)) 100%
      );
    color: var(--texte);
    border-color: var(--accent-ui);
  }
  .pad-grille {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 0.3rem;
  }
  .pad-touche {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 0.15rem;
    background: var(--btn-surface);
    box-shadow: var(--btn-inset);
    color: var(--texte);
    border: 1px solid var(--bordure-ui);
    border-radius: var(--rayon-petit);
    padding: 0.4rem 0;
    font: inherit;
    cursor: pointer;
    min-height: 3rem;
    transition: background 0.15s ease, box-shadow 0.15s ease, border-color 0.1s;
  }
  .pad-touche:hover {
    background: var(--btn-surface-hover);
    box-shadow: var(--btn-inset-hover);
    border-color: var(--accent-ui);
  }
  .pad-touche.selected {
    border-color: var(--accent-ui);
    box-shadow: 0 0 0 1px var(--accent-ui);
  }
  .pad-touche.reservee {
    opacity: 0.5;
    cursor: default;
  }
  .pad-touche.audio { background: color-mix(in srgb, var(--accent-ui) 15%, var(--fond-controle)); }
  .pad-touche.image { background: color-mix(in srgb, var(--message-ok-color) 15%, var(--fond-controle)); }
  .pad-touche.video { background: color-mix(in srgb, var(--accent-orange) 15%, var(--fond-controle)); }
  .pad-touche-nom {
    font-size: 1.1rem;
    font-weight: 600;
  }
  .pad-touche-badge {
    font-size: 0.65rem;
    text-transform: uppercase;
    opacity: 0.7;
    letter-spacing: 0.03em;
  }
  .pad-touche-badge.reservee {
    color: var(--texte);
  }
  .alerte-nom {
    flex: 1;
    font-size: 0.9rem;
    font-weight: 600;
  }
  .pad-erreur {
    margin: 0.5rem 0;
    padding: 0.6rem 0.9rem;
    background: color-mix(in srgb, var(--message-error-color) 15%, transparent);
    border: 1px solid color-mix(in srgb, var(--message-error-color) 60%, transparent);
    border-radius: 8px;
    color: var(--message-user-action-color);
    font-size: 0.85rem;
    font-weight: 600;
  }
  .alerte-toggle,
  .alerte-test {
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
  .alerte-toggle.on {
    background: var(--message-ok-color);
    color: #fff;
    border-color: var(--message-ok-color);
  }
  .alerte-test:hover,
  .alerte-toggle:hover {
    background: var(--btn-surface-hover);
    box-shadow: var(--btn-inset-hover);
    border-color: var(--accent-ui);
  }
  .alerte-toggle.on:hover {
    background: var(--message-ok-color);
    color: #fff;
  }
  .alerte-field {
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
    min-width: 0;
    margin-bottom: 0.4rem;
  }
  .alerte-field input {
    background: var(--fond-controle);
    color: var(--texte);
    border: 1px solid var(--bordure-ui);
    border-radius: var(--rayon-petit);
    padding: 0.25rem 0.4rem;
    font: inherit;
    font-size: 0.8rem;
    width: 100%;
  }
  .alerte-2col {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 0.4rem;
  }
  .alerte-4col {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 0.4rem;
  }
  .alerte-presets {
    display: grid;
    grid-template-columns: repeat(6, 1fr);
    gap: 0.25rem;
    margin-bottom: 0.4rem;
  }
  .alerte-presets .alerte-test {
    padding: 0.15rem 0;
    text-align: center;
  }
  .alerte-son {
    display: flex;
    align-items: center;
    gap: 0.4rem;
  }
  .alerte-son-nom {
    flex: 1;
    font-size: 0.75rem;
    opacity: 0.8;
    word-break: break-all;
  }

  /* Tabs — mêmes patterns que CadresModal (bordures var(--accent-ui), pas de radius) */
  .tabs {
    display: flex;
    flex-shrink: 0;
    gap: 0.2rem;
    border-bottom: 1px solid var(--bordure-ui);
    padding: 0 1rem;
  }
  .tab {
    background: var(--btn-surface);
    box-shadow: var(--btn-inset);
    color: var(--texte);
    border: none;
    border-bottom: 2px solid transparent;
    padding: 0.4rem 0.8rem;
    cursor: pointer;
    opacity: 0.6;
    transition: background 0.15s ease, box-shadow 0.15s ease, opacity 0.15s ease;
  }
  .tab:hover {
    background: var(--btn-surface-hover);
    box-shadow: var(--btn-inset-hover);
    opacity: 0.8;
  }
  .tab.active {
    opacity: 1;
    border-bottom-color: var(--accent-ui);
  }

  /* Corps : scroll, colonne de contenu contenue et centrée (pas étirée sur
     les 900px de la modale) */
  .tab-body {
    flex: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 0.8rem;
    padding: 1rem;
    max-width: 680px;
    width: 100%;
    margin: 0 auto;
  }
  /* Bloc = carte visuelle (bordure + radius + padding) — structure claire. */
  .bloc {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    border: 1px solid var(--bordure-ui);
    border-radius: var(--rayon);
    padding: 0.6rem 0.9rem;
    background: var(--fond-panneau);
  }
  .bloc-titre {
    font-size: 0.85rem;
    font-weight: 600;
    opacity: 0.85;
    border-bottom: 1px solid var(--bordure-ui);
    padding-bottom: 0.2rem;
  }
  /* Selecteur de police (bandeau) — même style que TitreEditForm. */
  .police-select {
    width: 100%;
    padding: 0.4rem 0.5rem;
    background: var(--fond-controle);
    color: var(--texte);
    border: 1px solid var(--bordure-ui);
    font: inherit;
    font-size: 0.85rem;
    cursor: pointer;
  }
  .police-select:focus {
    outline: none;
    border-color: var(--accent-ui);
  }
  .police-select option {
    padding: 0.3rem;
    background: var(--fond);
    color: var(--texte);
  }

  /* Checkbox — accent violet néon */
  .toggle {
    display: flex;
    align-items: center;
    gap: 0.3rem;
    cursor: pointer;
  }
  .toggle input[type="checkbox"] {
    accent-color: var(--accent-ui);
  }

  /* Slider durée — même style que CadresModal .range */
  .duree-row {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    margin-top: 0.4rem;
  }
  .duree-val {
    font-size: 0.85rem;
    font-variant-numeric: tabular-nums;
  }
  .range {
    width: 100%;
    height: 14px;
    cursor: pointer;
    accent-color: var(--accent-ui);
    background: var(--fond);
  }
  .position-row {
    display: flex;
    gap: 0.4rem;
  }
  .pos-btn {
    flex: 1;
    background: var(--btn-surface);
    box-shadow: var(--btn-inset);
    color: var(--texte);
    border: 1px solid var(--bordure-ui);
    padding: 0.4rem 0.6rem;
    cursor: pointer;
    font-size: 0.85rem;
    transition: background 0.15s ease, box-shadow 0.15s ease, border-color 0.15s ease;
  }
  .pos-btn:hover {
    background: var(--btn-surface-hover);
    box-shadow: var(--btn-inset-hover);
    border-color: var(--accent-ui);
  }
  .pos-btn.active {
    background:
      linear-gradient(
        180deg,
        color-mix(in srgb, var(--accent-ui) 25%, var(--fond-controle)) 0%,
        color-mix(in srgb, var(--accent-ui) 12%, var(--fond-controle)) 100%
      );
    color: var(--texte);
    border-color: var(--accent-ui);
    font-weight: 600;
  }
  .hint {
    font-size: 0.75rem;
    opacity: 0.5;
  }
  .field-label {
    font-size: 0.7rem;
    opacity: 0.6;
  }

  /* Mini-canvas overlay : drag/resize du squelette */
  .mini-canvas {
    position: relative;
    width: 100%;
    aspect-ratio: 16 / 9;
    background: var(--fond);
    border: 1px solid var(--bordure-ui);
    overflow: hidden;
    touch-action: none;
    user-select: none;
  }
  .overlay-skeleton {
    position: absolute;
    background: color-mix(in srgb, var(--accent-ui) 12%, transparent);
    border: 2px solid var(--bordure-ui-active);
    cursor: move;
    display: flex;
    align-items: center;
    justify-content: center;
    touch-action: none;
  }
  .overlay-skeleton:hover {
    background: color-mix(in srgb, var(--accent-ui) 18%, transparent);
  }
  .skeleton-label {
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

  /* File d'attente — boutons style CadresModal (border var(--accent-ui), hover accent) */
  .current {
    margin-bottom: 0.4rem;
    padding: 0.4rem;
    background: var(--fond-panneau);
    border: 1px solid var(--bordure-ui);
  }
  .current-label {
    font-size: 0.75rem;
    opacity: 0.7;
    margin-bottom: 0.2rem;
  }
  .queue-actions {
    display: flex;
    gap: 0.3rem;
    margin-bottom: 0.3rem;
  }
  .queue-actions button {
    background: var(--btn-surface);
    box-shadow: var(--btn-inset);
    color: var(--texte);
    border: 1px solid var(--bordure-ui);
    padding: 0.2rem 0.5rem;
    cursor: pointer;
    font-size: 0.8rem;
    transition: background 0.15s ease, box-shadow 0.15s ease, border-color 0.15s ease;
  }
  .queue-actions button:hover {
    background: var(--btn-surface-hover);
    box-shadow: var(--btn-inset-hover);
    border-color: var(--accent-ui);
  }
  .queue-actions button:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .queue-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
  }
  .item {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.3rem;
    border: 1px solid var(--bordure-ui);
  }
  .avatar {
    width: 24px;
    height: 24px;
    object-fit: cover;
    flex-shrink: 0;
  }
  .item-info {
    flex: 1;
    min-width: 0;
  }
  .item-pseudo {
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .item-clip {
    font-size: 0.75rem;
    opacity: 0.7;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .item-actions {
    display: flex;
    gap: 0.15rem;
    flex-shrink: 0;
  }
  .item-actions button {
    padding: 0.1rem 0.3rem;
    font-size: 0.75rem;
    cursor: pointer;
    background: var(--btn-surface);
    box-shadow: var(--btn-inset);
    color: var(--texte);
    border: 1px solid var(--bordure-ui);
    transition: background 0.15s ease, box-shadow 0.15s ease, border-color 0.15s ease;
  }
  .item-actions button:hover {
    background: var(--btn-surface-hover);
    box-shadow: var(--btn-inset-hover);
    border-color: var(--accent-ui);
  }
  .item-actions button:disabled {
    opacity: 0.3;
    cursor: default;
  }
  .empty {
    opacity: 0.5;
    font-style: italic;
    font-size: 0.85rem;
  }
  .actions-bar {
    display: flex;
    align-items: center;
    gap: 0.8rem;
    margin-bottom: 0.4rem;
  }
  .progress {
    font-size: 0.8rem;
    opacity: 0.8;
  }
  .attribution-body {
    display: flex;
    gap: 1rem;
  }
  .followers-list {
    flex: 1;
    overflow-y: auto;
    max-height: 240px;
    border-right: 1px solid var(--bordure-ui);
    padding-right: 0.6rem;
  }
  .followers-list h3 {
    font-size: 0.85rem;
    margin: 0 0 0.3rem;
  }
  .search-input {
    width: 100%;
    box-sizing: border-box;
    background: var(--fond-controle);
    color: var(--texte);
    border: 1px solid var(--bordure-ui);
    padding: 0.3rem 0.5rem;
    font: inherit;
    font-size: 0.8rem;
    margin-bottom: 0.4rem;
  }
  .search-input::placeholder {
    opacity: 0.4;
  }
  .search-input:focus {
    outline: none;
    border-color: var(--accent-ui);
  }
  .followers-list ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .followers-list li {
    display: flex;
    align-items: center;
    gap: 0.2rem;
    margin-bottom: 0.15rem;
  }
  .follower-row {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: space-between;
    background: var(--btn-surface);
    box-shadow: var(--btn-inset);
    border: 1px solid var(--bordure-ui);
    color: var(--texte);
    padding: 0.25rem 0.4rem;
    cursor: pointer;
    text-align: left;
    transition: background 0.15s ease, box-shadow 0.15s ease, border-color 0.15s ease;
  }
  .follower-row.selected {
    background:
      linear-gradient(
        180deg,
        color-mix(in srgb, var(--accent-ui) 25%, var(--fond-controle)) 0%,
        color-mix(in srgb, var(--accent-ui) 12%, var(--fond-controle)) 100%
      );
    color: var(--texte);
    border-color: var(--accent-ui);
  }
  .follower-row:hover {
    background: var(--btn-surface-hover);
    box-shadow: var(--btn-inset-hover);
    border-color: var(--accent-ui);
  }
  .login {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .badge {
    font-size: 0.75rem;
    flex-shrink: 0;
  }
  .badge.actif {
    color: var(--message-ok-color);
  }
  .badge.inactif {
    opacity: 0.4;
  }
  .mini-btn {
    background: var(--btn-surface);
    box-shadow: var(--btn-inset);
    color: var(--texte);
    border: 1px solid var(--bordure-ui);
    cursor: pointer;
    padding: 0.2rem 0.3rem;
    font-size: 0.8rem;
    transition: background 0.15s ease, box-shadow 0.15s ease, border-color 0.15s ease;
  }
  .mini-btn:hover {
    background: var(--btn-surface-hover);
    box-shadow: var(--btn-inset-hover);
    border-color: var(--accent-ui);
  }
  .clip-picker {
    flex: 1;
    overflow-y: auto;
    max-height: 240px;
    padding-left: 0.4rem;
  }
  .clip-picker h3 {
    font-size: 0.85rem;
    margin: 0 0 0.3rem;
  }
  .clips-list {
    list-style: none;
    margin: 0 0 0.4rem;
    padding: 0;
  }
  .clips-list li {
    margin-bottom: 0.3rem;
  }
  .clip-row {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.2rem;
    flex: 1;
    min-width: 0;
  }
  .clip-row.attribue {
    background: color-mix(in srgb, var(--accent-ui) 15%, var(--fond-controle));
    color: var(--texte);
  }
  .clip-actions {
    display: flex;
    gap: 0.2rem;
    flex-shrink: 0;
  }
  .thumb {
    width: 80px;
    height: 45px;
    object-fit: cover;
    flex-shrink: 0;
  }
  .clip-info {
    flex: 1;
    min-width: 0;
  }
  .clip-title {
    font-size: 0.85rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .clip-meta {
    font-size: 0.75rem;
    opacity: 0.6;
  }

  /* Boutons d'action — recette Aero */
  .action {
    background: var(--btn-surface);
    box-shadow: var(--btn-inset);
    color: var(--texte);
    border: 1px solid var(--bordure-ui);
    padding: 0.4rem 0.8rem;
    cursor: pointer;
    transition: background 0.15s ease, box-shadow 0.15s ease, border-color 0.15s ease;
  }
  .action:hover {
    background: var(--btn-surface-hover);
    box-shadow: var(--btn-inset-hover), var(--glow-ui);
    border-color: var(--accent-ui);
  }
  .action:disabled {
    opacity: 0.4;
    cursor: default;
  }

  /* Bouton "Tester" / "Stop" */
  .test-btn {
    background: var(--btn-surface);
    box-shadow: var(--btn-inset);
    color: var(--texte);
    border: 1px solid var(--bordure-ui);
    padding: 0.2rem 0.5rem;
    cursor: pointer;
    font-size: 0.75rem;
    flex-shrink: 0;
    margin-left: 0.3rem;
    transition: background 0.15s ease, box-shadow 0.15s ease, border-color 0.15s ease;
  }
  .test-btn:hover {
    background: var(--btn-surface-hover);
    box-shadow: var(--btn-inset-hover);
    border-color: var(--accent-ui);
  }
  .test-btn:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .test-btn.stop {
    background: var(--message-error-color);
    color: #fff;
    border-color: var(--message-error-color);
    font-weight: 600;
  }

  /* Scrollbars (variables CSS HUD de l'app).
     Appliquées aux 3 zones scrollables : .tab-body, .followers-list, .clip-picker. */
  .tab-body,
  .followers-list,
  .clip-picker {
    scrollbar-width: thin;
    scrollbar-color: var(--bordure-ui-active) var(--fond);
  }
  .tab-body::-webkit-scrollbar,
  .followers-list::-webkit-scrollbar,
  .clip-picker::-webkit-scrollbar {
    width: 8px;
  }
  .tab-body::-webkit-scrollbar-track,
  .followers-list::-webkit-scrollbar-track,
  .clip-picker::-webkit-scrollbar-track {
    background: var(--fond);
  }
  .tab-body::-webkit-scrollbar-thumb,
  .followers-list::-webkit-scrollbar-thumb,
  .clip-picker::-webkit-scrollbar-thumb {
    background: var(--bordure-ui-active);
    border: 2px solid var(--fond);
  }
  .tab-body::-webkit-scrollbar-thumb:hover,
  .followers-list::-webkit-scrollbar-thumb:hover,
  .clip-picker::-webkit-scrollbar-thumb:hover {
    background: var(--accent-ui);
  }
</style>
