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
    bandeauResetSession,
    bandeauTester,
  } from "../stores/bandeau";
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
    LIBELLES_RESERVEES,
    NB_PLAGES,
  } from "../stores/padNumerique";
  import { sceneStore } from "../stores/scene";
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

  function cfgType(t: TypeAlerte): AlerteTypeConfig {
    return configType(alertes, t);
  }
  function majAlerte(t: TypeAlerte, patch: Partial<AlerteTypeConfig>) {
    void sauverConfigType(t, { ...cfgType(t), ...patch });
  }
  function toggleAlerteActif(t: TypeAlerte) {
    majAlerte(t, { actif: !cfgType(t).actif });
  }
  async function testerAlerte(t: TypeAlerte) {
    try {
      await tauri.alertesTester(t);
    } catch (e) {
      console.warn("alertesTester:", e);
    }
  }
  async function importerSonAlerte(t: TypeAlerte) {
    try {
      const rel = await tauri.importSon();
      if (rel) majAlerte(t, { son: rel });
    } catch (e) {
      alert("Import refusé : " + e);
    }
  }
  // Mode média effectif du type : "aucun" | "image" | "video".
  function modeMedia(t: TypeAlerte): "aucun" | "image" | "video" {
    const cfg = cfgType(t);
    if (cfg.media && cfg.media_kind === "video") return "video";
    if (cfg.media) return "image";
    return "aucun";
  }
  async function importerMediaAlerte(t: TypeAlerte, kind: "image" | "video") {
    try {
      const rel = await tauri.importAlerteMedia(kind);
      if (rel) majAlerte(t, { media: rel, media_kind: kind });
    } catch (e) {
      alert("Import refusé : " + e);
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
      texte_template: "{pseudo} lance {commande} !",
      couleur: "#ffffff",
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
      alert("Import refusé : " + e);
    }
  }
  async function importerSonCommande() {
    try {
      const rel = await tauri.importSon();
      if (rel) majCommande({ son: rel });
    } catch (e) {
      alert("Import refusé : " + e);
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
  function modeMediaPad(t: ToucheConfig | null): "aucun" | "audio" | "image" | "video" {
    if (!t) return "aucun";
    if (t.media_type === "audio") return "audio";
    if (t.media_type === "video") return "video";
    if (t.media_type === "image") return "image";
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
      alert("Import refusé : " + String(e));
    }
  }

  /// Importe un son pour la touche sélectionnée (audio pur OU son d'image).
  async function onImporterSonPad(): Promise<void> {
    if (!padSelCode) return;
    try {
      await padImporterSon(padSelCode, padPlage);
    } catch (e) {
      alert("Import refusé : " + String(e));
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
    } catch (e) {
      console.error("[Welcome] lister clips streamer:", String(e));
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
    } catch (e) {
      console.warn("[Welcome] résolution MP4 à l'attribution échouée (fallback à la volée) :", String(e));
    }

    const config: WelcomeViewerConfig = {
      actif: true,
      clip_id: clip.id,
      clip_titre: clip.title,
      clip_thumbnail: clip.thumbnail_url,
      clip_mp4_url: mp4Url,
      clip_duree_ms: Math.round(clip.duration * 1000),
      message: `Bienvenue ${login} !`,
      display_name: login,
      avatar: null,
    };
    try {
      await welcomeSauverViewer(login, config);
      console.log("[Welcome] attribué manuel:", login, clip.title);
    } catch (e) {
      console.error("[Welcome] attribuer manuel:", String(e));
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
      console.log("[Welcome] test clip lancé:", clip.title);
    } catch (e) {
      console.error("[Welcome] test clip:", String(e));
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
      const result = await welcomeAttribuerAuto(followersInput);
      console.log("[Welcome] attribution auto terminée:", result);
    } catch (e) {
      console.error("[Welcome] attribution auto:", String(e));
    }
    attributing = false;
  }

  async function onSupprimer(login: string) {
    try {
      await welcomeSupprimerViewer(login);
    } catch (e) {
      console.error("[Welcome] supprimer viewer:", String(e));
    }
  }

  async function onToggleActif(login: string) {
    const config = configPourLogin(login);
    if (!config) return;
    try {
      await welcomeSauverViewer(login, { ...config, actif: !config.actif });
    } catch (e) {
      console.error("[Welcome] toggle actif:", String(e));
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
    } catch (e) {
      console.error("[PositionOverlay] set config:", String(e));
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

  async function onBandeauDureeInput(e: Event) {
    const sec = parseFloat((e.target as HTMLInputElement).value);
    await bandeauSetDuree(Math.round(sec * 1000));
  }

  async function onBandeauTester() {
    await bandeauTester("ViewerTest", "Ceci est mon premier message !");
  }
</script>

<Modal title="Interactions chat" onClose={onFermer}>
  <div class="tabs">
    <button class="tab" class:active={tab === "bienvenue"} onclick={() => (tab = "bienvenue")}>
      Clip de bienvenue
    </button>
    <button class="tab" class:active={tab === "bandeau"} onclick={() => (tab = "bandeau")}>
      Bandeau premier message
    </button>
    <button class="tab" class:active={tab === "alertes"} onclick={() => (tab = "alertes")}>
      Alertes
    </button>
    <button class="tab" class:active={tab === "commandes"} onclick={() => (tab = "commandes")}>
      Commandes
    </button>
    <button
      class="tab"
      class:active={tab === "pad"}
      onclick={() => { tab = "pad"; void chargerPad(); }}
    >
      Pad numérique
    </button>
    <button class="tab" class:active={tab === "position"} onclick={() => (tab = "position")}>
      Squelette de position
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
              <span class="alerte-nom">Général</span>
            </button>
            <button
              class="alerte-select"
              class:selected={secBienvenue === "file"}
              onclick={() => (secBienvenue = "file")}
            >
              <span class="alerte-nom">File d'attente</span>
            </button>
            <button
              class="alerte-select"
              class:selected={secBienvenue === "attribution"}
              onclick={() => (secBienvenue = "attribution")}
            >
              <span class="alerte-nom">Attribution</span>
            </button>
          </div>

          <!-- Panneau droit : contenu de la section sélectionnée -->
          <div class="alerte-detail">
            {#if secBienvenue === "general"}
              <!-- Config globale + overlay -->
              <div class="bloc">
                <div class="bloc-titre">Général</div>
                <label class="toggle">
                  <input
                    type="checkbox"
                    checked={actif}
                    onchange={(e) => welcomeSetConfigGlobaleActif(e.currentTarget.checked)}
                  />
                  <span>Clips de bienvenue activés</span>
                </label>

                <div class="duree-row">
                  <span class="field-label">Durée d'affichage</span>
                  <span class="duree-val">
                    {dureeAffichageSec === 0 ? "Naturelle" : `${dureeAffichageSec}s`}
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
                <p class="hint">0 = durée naturelle du clip · 1-30s = forcé</p>
              </div>
            {/if}

            {#if secBienvenue === "file"}
              <!-- File d'attente -->
              <div class="bloc">
                <div class="bloc-titre">File d'attente ({queue.length})</div>

                {#if current}
                  <div class="current">
                    <div class="current-label">En cours</div>
                    <div class="item">
                      {#if current.avatar}
                        <img src={current.avatar} alt="" class="avatar" />
                      {/if}
                      <div class="item-info">
                        <div class="item-pseudo">{current.display_name}</div>
                        <div class="item-clip">{current.clip_titre}</div>
                      </div>
                      <div class="item-actions">
                        <button title="Skip" onclick={() => welcomeSkip()}>⏭</button>
                        <button title="Stop tout" onclick={() => welcomeStop()}>⏹</button>
                      </div>
                    </div>
                  </div>
                {/if}

                <div class="queue-actions">
                  <button onclick={() => welcomeVider()} disabled={queue.length === 0}>
                    Vider
                  </button>
                  <button onclick={() => welcomeResetSession()}>
                    Reset session
                  </button>
                </div>

                {#if queue.length === 0}
                  <p class="empty">Aucun clip en attente</p>
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
                          <button title="Remonter" onclick={() => welcomeRemonter(item.id)} disabled={i === 0}>↑</button>
                          <button title="Descendre" onclick={() => welcomeDescendre(item.id)} disabled={i === queue.length - 1}>↓</button>
                          <button title="Retirer" onclick={() => welcomeRetirer(item.id)}>✕</button>
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
                <div class="bloc-titre">Attribution (followers Twitch)</div>
          <div class="actions-bar">
            <button
              class="action"
              onclick={onAttribuerAuto}
              disabled={attributing || listeAvecStreamer.length === 0}
            >
              {attributing ? "Attribution en cours..." : "Attribution auto (tous les followers)"}
            </button>
            {#if progress}
              <span class="progress">
                {progress.succes} succès / {progress.echecs} échecs — {progress.login}
              </span>
            {/if}
          </div>

          <div class="attribution-body">
            <div class="followers-list">
              <h3>Followers ({listeAvecStreamer.length})</h3>
              <input
                class="search-input"
                type="text"
                placeholder="Rechercher un follower..."
                bind:value={searchQuery}
              />
              {#if listeAvecStreamer.length === 0}
                <p class="empty">Aucun follower. Connecter Twitch + charger la communauté.</p>
              {:else if followersFiltres.length === 0}
                <p class="empty">Aucun follower ne correspond à « {searchQuery} ».</p>
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
                        <button class="mini-btn" title={config.actif ? "Désactiver" : "Activer"} onclick={() => onToggleActif(f.login)}>
                          {config.actif ? "⏸" : "▶"}
                        </button>
                        <button class="mini-btn" title="Supprimer" onclick={() => onSupprimer(f.login)}>✕</button>
                      {/if}
                    </li>
                  {/each}
                </ul>
              {/if}
            </div>

            {#if selectedLogin}
              <div class="clip-picker">
                <h3>Clip de {selectedLogin}</h3>
                {#if loadingClips}
                  <p class="empty">Chargement des clips...</p>
                {:else if clipsStreamer.length === 0}
                  <p class="empty">Aucun clip disponible pour ce streamer.</p>
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
                            title="Attribuer ce clip à ce viewer"
                            onclick={() => onAttribuerManuel(clip)}
                          >
                            Attribuer
                          </button>
                          {#if testingClipId === clip.id}
                            <button
                              class="test-btn stop"
                              title="Arrêter le clip"
                              onclick={onStopperTest}
                            >
                              ⏹ Stop
                            </button>
                          {:else}
                            <button
                              class="test-btn"
                              title="Tester ce clip côté diffusion"
                              onclick={() => onTesterClip(clip)}
                              disabled={!!testingClipId}
                            >
                              Tester
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
              <span class="alerte-nom">Général</span>
            </button>
            <button
              class="alerte-select"
              class:selected={secBandeau === "duree"}
              onclick={() => (secBandeau = "duree")}
            >
              <span class="alerte-nom">Durée</span>
            </button>
            <button
              class="alerte-select"
              class:selected={secBandeau === "position"}
              onclick={() => (secBandeau = "position")}
            >
              <span class="alerte-nom">Position</span>
            </button>
            <button
              class="alerte-select"
              class:selected={secBandeau === "test"}
              onclick={() => (secBandeau = "test")}
            >
              <span class="alerte-nom">Test &amp; session</span>
            </button>
            <button
              class="alerte-select"
              class:selected={secBandeau === "apparence"}
              onclick={() => (secBandeau = "apparence")}
            >
              <span class="alerte-nom">Apparence</span>
            </button>
          </div>

          <!-- Panneau droit : contenu de la section -->
          <div class="alerte-detail">
            {#if secBandeau === "general"}
              <div class="bloc">
                <div class="bloc-titre">Général</div>
                <label class="toggle">
                  <input
                    type="checkbox"
                    checked={bActif}
                    onchange={(e) => bandeauSetActif(e.currentTarget.checked)}
                  />
                  <span>Bandeau premier message activé</span>
                </label>
                <p class="hint">
                  Affiche un bandeau en {bPosition === "haut" ? "haut" : "bas"} d'écran
                  au premier message de chaque viewer (toutes plateformes).
                </p>
              </div>
            {:else if secBandeau === "duree"}
              <div class="bloc">
                <div class="bloc-titre">Durée d'affichage</div>
                <div class="duree-row">
                  <span class="field-label">Durée</span>
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
                <p class="hint">2-20s · le bandeau s'auto-masque à la fin</p>
              </div>
            {:else if secBandeau === "position"}
              <div class="bloc">
                <div class="bloc-titre">Position</div>
                <div class="position-row">
                  <button
                    class="pos-btn"
                    class:active={bPosition === "bas"}
                    onclick={() => bandeauSetPosition("bas")}
                  >
                    ▾ Bas
                  </button>
                  <button
                    class="pos-btn"
                    class:active={bPosition === "haut"}
                    onclick={() => bandeauSetPosition("haut")}
                  >
                    ▴ Haut
                  </button>
                </div>
              </div>
            {:else if secBandeau === "test"}
              <div class="bloc">
                <div class="bloc-titre">Test &amp; session</div>
                <div class="queue-actions">
                  <button onclick={onBandeauTester} disabled={!bActif}>
                    Tester le bandeau
                  </button>
                  <button onclick={() => bandeauResetSession()}>
                    Reset session
                  </button>
                </div>
                <p class="hint">
                  « Tester » affiche un bandeau fictif côté diffusion. « Reset session »
                  rend tous les viewers à nouveau éligibles (nouveau stream).
                </p>
              </div>
            {:else}
              <div class="bloc">
                <div class="bloc-titre">Apparence — couleurs du cadre</div>
                <p class="hint">
                  Le bandeau reprend les couleurs du cadre widget appliqué à la scène
                  (voir modale Cadres). Sans cadre actif, le thème noir &amp; blanc
                  par défaut est utilisé.
                </p>
              </div>
            {/if}
          </div>
        </div>
      </div>
    {/if}

    {#if tab === "alertes"}
      <div class="tab-body">
        <p class="hint">
          Twitch. Les nouveaux followers sont détectés automatiquement
          (vérification toutes les 60 s). Sélectionnez un type à gauche pour
          régler son média, son texte et son comportement. La position/taille
          de l'overlay est réglée dans l'onglet « Squelette de position »
          (partagée avec les clips de bienvenue).
        </p>

        <div class="alertes-layout">
          <!-- Colonne gauche : liste des types (nom + ON/OFF) -->
          <div class="alertes-liste">
            {#each TYPES_ALERTE as t (t)}
              {@const c = cfgType(t)}
              <div class="alerte-item" class:selected={typeActif === t}>
                <button
                  class="alerte-select"
                  class:selected={typeActif === t}
                  onclick={() => (typeActif = t)}
                >
                  <span class="alerte-nom">{NOMS_ALERTE[t]}</span>
                </button>
                <button
                  class="alerte-toggle"
                  class:on={c.actif}
                  onclick={() => toggleAlerteActif(t)}
                  title={c.actif ? "Désactiver ce type d'alerte" : "Activer ce type d'alerte"}
                >{c.actif ? "ON" : "OFF"}</button>
              </div>
            {/each}
          </div>

          <!-- Panneau droit : config du type sélectionné -->
          <div class="alerte-detail">
            <div class="bloc">
              <div class="bloc-titre">Média</div>
              <div class="media-modes">
                <button
                  class="mode-btn"
                  class:active={modeMedia(typeActif) === "aucun"}
                  onclick={() => majAlerte(typeActif, { media: undefined, media_kind: undefined, son: undefined })}
                >Aucun</button>
                <button
                  class="mode-btn"
                  class:active={modeMedia(typeActif) === "image"}
                  onclick={() => importerMediaAlerte(typeActif, "image")}
                >Image</button>
                <button
                  class="mode-btn"
                  class:active={modeMedia(typeActif) === "video"}
                  onclick={() => importerMediaAlerte(typeActif, "video")}
                >Vidéo</button>
              </div>
              {#if cfgSel.media}
                <div class="alerte-son">
                  <span class="alerte-son-nom">{cfgSel.media.split("/").pop()}</span>
                  <button
                    class="alerte-toggle"
                    onclick={() => majAlerte(typeActif, { media: undefined, media_kind: undefined })}
                    title="Retirer le média"
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
                      title="Retirer le son"
                    >✕</button>
                  {:else}
                    <button class="alerte-test" onclick={() => importerSonAlerte(typeActif)}>
                      Importer un son
                    </button>
                  {/if}
                </div>
              {/if}
              <p class="hint">
                Image = image + son + texte · Vidéo = vidéo seule (son inclus) +
                texte · Aucun = icône + texte.
              </p>
            </div>

            <div class="bloc">
              <div class="bloc-titre">Texte</div>
              <label class="alerte-field">
                <span class="field-label">Texte</span>
                <input
                  value={cfgSel.texte_template}
                  spellcheck="false"
                  onchange={(e) => majAlerte(typeActif, { texte_template: (e.target as HTMLInputElement).value })}
                />
              </label>
              <p class="hint">
                Variables : {'{pseudo} {nbViewers} {nbBits} {niveauSub} {nbMoisCumul} {destinataire}'}
              </p>
              <p class="hint">
                Variables : {'{pseudo} {nbViewers} {nbBits} {niveauSub} {nbMoisCumul} {destinataire}'}
              </p>
              <p class="hint">
                La couleur du texte suit le cadre SVG de la scène (modale
                « Vos cadres »). Sans cadre actif : blanc.
              </p>
              <label class="alerte-field">
                <span class="field-label">Taille {cfgSel.taille_px}px</span>
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
              <div class="bloc-titre">Comportement</div>
              <div class="alerte-2col">
                <label class="alerte-field">
                  <span class="field-label">Durée (s)</span>
                  <input
                    type="number"
                    min="1"
                    max="60"
                    value={Math.round(cfgSel.duree_ms / 1000)}
                    onchange={(e) => majAlerte(typeActif, { duree_ms: Math.max(1, +(e.target as HTMLInputElement).value) * 1000 })}
                  />
                </label>
                <label class="alerte-field">
                  <span class="field-label">Cooldown global (s)</span>
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
                <span class="field-label">Cooldown par viewer (s)</span>
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
              <div class="bloc-titre">Test</div>
              <button class="alerte-test" onclick={() => testerAlerte(typeActif)} disabled={!cfgSel.actif}>
                Tester l'alerte côté diffusion
              </button>
              <p class="hint">
                Le Test contourne les cooldowns. Sans média : icône + texte.
              </p>
            </div>
          </div>
        </div>
      </div>
    {/if}

    {#if tab === "commandes"}
      <div class="tab-body">
        <p class="hint">
          Commandes tapées dans le chat (ex: <b>!hype</b>). Quand un viewer
          tape la commande, elle s'affiche en overlay sur la diffusion (même
          moteur que les alertes : file d'attente, cooldowns, cadre SVG).
          La position/taille est réglée dans l'onglet « Squelette de position ».
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
                  title={c.actif ? "Désactiver cette commande" : "Activer cette commande"}
                >{c.actif ? "ON" : "OFF"}</button>
              </div>
            {/each}
            <button class="alerte-test cmd-ajouter" onclick={onAjouterCommande}>
              + Ajouter
            </button>
          </div>

          <!-- Panneau droit : config de la commande sélectionnée -->
          <div class="alerte-detail">
            {#if cmdSel}
              <div class="bloc">
                <div class="bloc-titre">Commande</div>
                <label class="alerte-field">
                  <span class="field-label">Commande (sans le !)</span>
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
                  Les viewers tapent <b>!{cmdSel.commande || "…"}</b> dans le
                  chat pour la déclencher.
                </p>
              </div>

              <div class="bloc">
                <div class="bloc-titre">Média</div>
                <div class="media-modes">
                  <button
                    class="mode-btn"
                    class:active={modeMediaCommande(cmdSel) === "aucun"}
                    onclick={() => majCommande({ media: undefined, media_kind: undefined, son: undefined })}
                  >Aucun</button>
                  <button
                    class="mode-btn"
                    class:active={modeMediaCommande(cmdSel) === "image"}
                    onclick={() => importerMediaCommande("image")}
                  >Image</button>
                  <button
                    class="mode-btn"
                    class:active={modeMediaCommande(cmdSel) === "video"}
                    onclick={() => importerMediaCommande("video")}
                  >Vidéo</button>
                </div>
                {#if cmdSel.media}
                  <div class="alerte-son">
                    <span class="alerte-son-nom">{cmdSel.media.split("/").pop()}</span>
                    <button
                      class="alerte-toggle"
                      onclick={() => majCommande({ media: undefined, media_kind: undefined })}
                      title="Retirer le média"
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
                        title="Retirer le son"
                      >✕</button>
                    {:else}
                      <button class="alerte-test" onclick={importerSonCommande}>
                        Importer un son
                      </button>
                    {/if}
                  </div>
                {/if}
                <p class="hint">
                  Image = image + son + texte · Vidéo = vidéo seule (son inclus)
                  + texte · Aucun = icône + texte.
                </p>
              </div>

              <div class="bloc">
                <div class="bloc-titre">Texte</div>
                <label class="alerte-field">
                  <span class="field-label">Texte</span>
                  <input
                    value={cmdSel.texte_template}
                    spellcheck="false"
                    onchange={(e) => majCommande({ texte_template: (e.target as HTMLInputElement).value })}
                  />
                </label>
                <p class="hint">
                  Variables : {'{pseudo} {commande} {message}'} — {'{message}'} =
                  texte tapé après la commande.
                </p>
                <p class="hint">
                  La couleur du texte suit le cadre SVG de la scène (modale
                  « Vos cadres »). Sans cadre actif : blanc.
                </p>
                <label class="alerte-field">
                  <span class="field-label">Taille {cmdSel.taille_px}px</span>
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
                <div class="bloc-titre">Comportement</div>
                <div class="alerte-2col">
                  <label class="alerte-field">
                    <span class="field-label">Durée (s)</span>
                    <input
                      type="number"
                      min="1"
                      max="60"
                      value={Math.round(cmdSel.duree_ms / 1000)}
                      onchange={(e) => majCommande({ duree_ms: Math.max(1, +(e.target as HTMLInputElement).value) * 1000 })}
                    />
                  </label>
                  <label class="alerte-field">
                    <span class="field-label">Cooldown global (s)</span>
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
                  <span class="field-label">Cooldown par viewer (s)</span>
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
                <div class="bloc-titre">Test & suppression</div>
                <div class="queue-actions">
                  <button class="alerte-test" onclick={() => testerCommande(cmdSel.id)} disabled={!cmdSel.actif}>
                    Tester la commande
                  </button>
                  <button
                    class="alerte-toggle"
                    onclick={() => { void supprimerCommande(cmdSel.id); }}
                    title="Supprimer cette commande"
                  >Supprimer</button>
                </div>
                <p class="hint">
                  Le Test contourne les cooldowns. Sans média : icône + texte.
                </p>
              </div>
            {:else}
              <div class="bloc">
                <p class="hint">Aucune commande sélectionnée — créez-en une.</p>
              </div>
          {/if}
          </div>
        </div>
      </div>
    {/if}

    {#if tab === "pad"}
      <div class="tab-body">
        <p class="hint">
          Pad numérique : 16 touches Numpad × 3 plages. Appuyez sur une touche
          du pavé numérique pour déclencher un son, une image ou une vidéo en
          overlay sur la diffusion. <b>+</b> et <b>-</b> du numpad changent de
          plage. La position/taille est réglée dans l'onglet « Squelette de
          position » (partagé avec welcome/alertes/commandes). L'audio est joué
          des deux côtés : diffusion (OBS capte pour les viewers) + dashboard
          (vous entendez sur votre PC).
        </p>

        {#if padErr}
          <div class="pad-erreur">⚠ {padErr}</div>
        {/if}

        <div class="alertes-layout">
          <!-- Colonne gauche : activation + plages + grille 4×4 -->
          <div class="alertes-liste">
            <div class="bloc">
              <div class="bloc-titre">Activation</div>
              <button
                class="alerte-toggle"
                class:on={padActif}
                onclick={() => void padSetActif(!padActif)}
                title={padActif ? "Désactiver le pad" : "Activer le pad"}
              >{padActif ? "ON" : "OFF"}</button>
              <p class="hint">
                {padActif
                  ? "Capture clavier globale active (GetAsyncKeyState 60Hz)."
                  : "Capture clavier globale inactive."}
              </p>
            </div>

            <div class="bloc">
              <div class="bloc-titre">Plages</div>
              <div class="pad-plages">
                {#each Array(NB_PLAGES) as _, i}
                  <button
                    class="pad-plage-btn"
                    class:active={padPlage === i}
                    onclick={() => void padSetPlage(i)}
                  >Plage {i + 1}</button>
                {/each}
              </div>
              <p class="hint">
                Plage courante : <b>{padPlage + 1}</b> / {NB_PLAGES}.
                Changez aussi avec <b>+</b> / <b>-</b> du numpad.
              </p>
            </div>

            <div class="bloc">
              <div class="bloc-titre">Touches (plage {padPlage + 1})</div>
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
                      title={reservee ? LIBELLES_RESERVEES[code] ?? code : code}
                    >
                      <span class="pad-touche-nom">{NOMS_AFFICHAGE[code] ?? code}</span>
                      {#if reservee}
                        <span class="pad-touche-badge reservee">réservée</span>
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
                <div class="bloc-titre">Touche {NOMS_AFFICHAGE[padSelCode] ?? padSelCode} (plage {padPlage + 1})</div>
                <p class="hint">
                  Appuyez sur cette touche du numpad pour déclencher le média.
                </p>
              </div>

              <div class="bloc">
                <div class="bloc-titre">Média</div>
                <div class="media-modes">
                  <button
                    class="mode-btn"
                    class:active={modeMediaPad(padSel) === "aucun"}
                    onclick={() => majPad({ media: undefined, media_kind: undefined, media_type: undefined, son: undefined })}
                  >Aucun</button>
                  <button
                    class="mode-btn"
                    class:active={modeMediaPad(padSel) === "audio"}
                    onclick={onImporterSonPad}
                  >Audio</button>
                  <button
                    class="mode-btn"
                    class:active={modeMediaPad(padSel) === "image"}
                    onclick={() => onImporterMediaPad("image")}
                  >Image</button>
                  <button
                    class="mode-btn"
                    class:active={modeMediaPad(padSel) === "video"}
                    onclick={() => onImporterMediaPad("video")}
                  >Vidéo</button>
                </div>

                {#if padSel.media}
                  <div class="alerte-son">
                    <span class="alerte-son-nom">{padSel.media.split("/").pop()}</span>
                    <button
                      class="alerte-toggle"
                      onclick={onRetirerMediaPad}
                      title="Retirer le média"
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
                        title="Retirer le son"
                      >✕</button>
                    {:else}
                      <button class="alerte-test" onclick={onImporterSonPad}>
                        Importer un son
                      </button>
                    {/if}
                  </div>
                {/if}

                <p class="hint">
                  Audio = son seul (joué diffusion + dashboard) ·
                  Image = image + son séparé · Vidéo = vidéo (son inclus, diffusion).
                </p>
              </div>

              <div class="bloc">
                <div class="bloc-titre">Comportement</div>
                <div class="alerte-2col">
                  <label class="alerte-field">
                    <span class="field-label">Durée (s)</span>
                    <input
                      type="number"
                      min="1"
                      max="60"
                      value={Math.round(padSel.duree_ms / 1000)}
                      onchange={(e) => majPad({ duree_ms: Math.max(1, +(e.target as HTMLInputElement).value) * 1000 })}
                    />
                  </label>
                  <label class="alerte-field">
                    <span class="field-label">Volume {Math.round(padSel.volume * 100)}%</span>
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
                <div class="bloc-titre">Test & suppression</div>
                <div class="queue-actions">
                  <button class="alerte-test" onclick={onTesterTouchePad} disabled={!padActif && modeMediaPad(padSel) === "aucun"}>
                    Tester la touche
                  </button>
                  <button
                    class="alerte-toggle"
                    onclick={onSupprimerTouchePad}
                    title="Réinitialiser cette touche"
                  >Réinitialiser</button>
                </div>
                <p class="hint">
                  Le Test déclenche la touche côté diffusion + joue l'audio local.
                </p>
              </div>
            {:else}
              <div class="bloc">
                <p class="hint">
                  Sélectionnez une touche dans la grille à gauche pour la
                  configurer. Les touches <b>+</b> et <b>-</b> sont réservées
                  (navigation entre plages).
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
          Position et taille du squelette partagé par les overlays « Clip de
          bienvenue » et « Alertes ». Glisser pour déplacer, coins pour
          redimensionner. La position s'applique en live sur la diffusion.
        </p>

        <div class="bloc">
          <div class="bloc-titre">Squelette de position</div>
          <div class="alerte-presets">
            {#each Object.entries(PRESETS_OVERLAY) as [pos, label] (pos)}
              <button class="alerte-test" title={pos} onclick={() => presetOverlay(pos)}>{label}</button>
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
              <span class="skeleton-label">Overlay</span>
              <div class="sk-handle nw" onpointerdown={(e) => onHandleDown(e, "resize-nw")} role="presentation"></div>
              <div class="sk-handle ne" onpointerdown={(e) => onHandleDown(e, "resize-ne")} role="presentation"></div>
              <div class="sk-handle sw" onpointerdown={(e) => onHandleDown(e, "resize-sw")} role="presentation"></div>
              <div class="sk-handle se" onpointerdown={(e) => onHandleDown(e, "resize-se")} role="presentation"></div>
            </div>
          </div>
          <p class="hint">
            Glisser pour déplacer · coins pour redimensionner · canvas {REF_W}×{REF_H}
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
              <span class="field-label">Largeur</span>
              <input
                type="number"
                min="80"
                max="1920"
                value={Math.round(ovW)}
                onchange={(e) => { ovW = Math.max(80, +(e.target as HTMLInputElement).value); void onOverlayCommit(); }}
              />
            </label>
            <label class="alerte-field">
              <span class="field-label">Hauteur (réf.)</span>
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
    border: 1px solid var(--bordure);
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
    border-color: var(--accent-violet);
  }
  .alerte-select.selected {
    background:
      linear-gradient(
        180deg,
        color-mix(in srgb, var(--accent-violet) 25%, var(--fond-controle)) 0%,
        color-mix(in srgb, var(--accent-violet) 12%, var(--fond-controle)) 100%
      );
    color: var(--texte);
    border-color: var(--accent-violet);
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
    border: 1px solid var(--bordure);
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
    border-color: var(--accent-violet);
  }
  .mode-btn.active {
    background:
      linear-gradient(
        180deg,
        color-mix(in srgb, var(--accent-violet) 25%, var(--fond-controle)) 0%,
        color-mix(in srgb, var(--accent-violet) 12%, var(--fond-controle)) 100%
      );
    color: var(--texte);
    border-color: var(--accent-violet);
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
    border: 1px solid var(--bordure);
    border-radius: var(--rayon-petit);
    padding: 0.3rem 0;
    font: inherit;
    cursor: pointer;
    transition: background 0.15s ease, box-shadow 0.15s ease, border-color 0.15s ease;
  }
  .pad-plage-btn:hover {
    background: var(--btn-surface-hover);
    box-shadow: var(--btn-inset-hover);
    border-color: var(--accent-violet);
  }
  .pad-plage-btn.active {
    background:
      linear-gradient(
        180deg,
        color-mix(in srgb, var(--accent-violet) 25%, var(--fond-controle)) 0%,
        color-mix(in srgb, var(--accent-violet) 12%, var(--fond-controle)) 100%
      );
    color: var(--texte);
    border-color: var(--accent-violet);
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
    border: 1px solid var(--bordure);
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
    border-color: var(--accent-violet);
  }
  .pad-touche.selected {
    border-color: var(--accent-violet);
    box-shadow: 0 0 0 1px var(--accent-violet);
  }
  .pad-touche.reservee {
    opacity: 0.5;
    cursor: default;
  }
  .pad-touche.audio { background: color-mix(in srgb, var(--accent-violet) 15%, var(--fond-controle)); }
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
    border: 1px solid var(--bordure);
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
    border-color: var(--accent-violet);
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
    border: 1px solid var(--bordure);
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

  /* Tabs — mêmes patterns que CadresModal (bordures var(--accent-violet), pas de radius) */
  .tabs {
    display: flex;
    flex-shrink: 0;
    gap: 0.2rem;
    border-bottom: 1px solid var(--bordure);
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
    border-bottom-color: var(--accent-violet);
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
    border: 1px solid var(--bordure);
    border-radius: var(--rayon);
    padding: 0.6rem 0.9rem;
    background: var(--fond-panneau);
  }
  .bloc-titre {
    font-size: 0.85rem;
    font-weight: 600;
    opacity: 0.85;
    border-bottom: 1px solid var(--bordure);
    padding-bottom: 0.2rem;
  }

  /* Checkbox — accent violet néon */
  .toggle {
    display: flex;
    align-items: center;
    gap: 0.3rem;
    cursor: pointer;
  }
  .toggle input[type="checkbox"] {
    accent-color: var(--accent-violet);
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
    accent-color: var(--accent-violet);
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
    border: 1px solid var(--bordure);
    padding: 0.4rem 0.6rem;
    cursor: pointer;
    font-size: 0.85rem;
    transition: background 0.15s ease, box-shadow 0.15s ease, border-color 0.15s ease;
  }
  .pos-btn:hover {
    background: var(--btn-surface-hover);
    box-shadow: var(--btn-inset-hover);
    border-color: var(--accent-violet);
  }
  .pos-btn.active {
    background:
      linear-gradient(
        180deg,
        color-mix(in srgb, var(--accent-violet) 25%, var(--fond-controle)) 0%,
        color-mix(in srgb, var(--accent-violet) 12%, var(--fond-controle)) 100%
      );
    color: var(--texte);
    border-color: var(--accent-violet);
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
    border: 1px solid var(--bordure);
    overflow: hidden;
    touch-action: none;
    user-select: none;
  }
  .overlay-skeleton {
    position: absolute;
    background: color-mix(in srgb, var(--accent-violet) 12%, transparent);
    border: 2px solid var(--bordure-active);
    cursor: move;
    display: flex;
    align-items: center;
    justify-content: center;
    touch-action: none;
  }
  .overlay-skeleton:hover {
    background: color-mix(in srgb, var(--accent-violet) 18%, transparent);
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
    background: var(--accent-violet);
    border: 1px solid var(--fond);
  }
  .sk-handle.nw { top: -6px; left: -6px; cursor: nwse-resize; }
  .sk-handle.ne { top: -6px; right: -6px; cursor: nesw-resize; }
  .sk-handle.sw { bottom: -6px; left: -6px; cursor: nesw-resize; }
  .sk-handle.se { bottom: -6px; right: -6px; cursor: nwse-resize; }

  /* File d'attente — boutons style CadresModal (border var(--accent-violet), hover accent) */
  .current {
    margin-bottom: 0.4rem;
    padding: 0.4rem;
    background: var(--fond-panneau);
    border: 1px solid var(--bordure);
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
    border: 1px solid var(--bordure);
    padding: 0.2rem 0.5rem;
    cursor: pointer;
    font-size: 0.8rem;
    transition: background 0.15s ease, box-shadow 0.15s ease, border-color 0.15s ease;
  }
  .queue-actions button:hover {
    background: var(--btn-surface-hover);
    box-shadow: var(--btn-inset-hover);
    border-color: var(--accent-violet);
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
    border: 1px solid var(--bordure);
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
    border: 1px solid var(--bordure);
    transition: background 0.15s ease, box-shadow 0.15s ease, border-color 0.15s ease;
  }
  .item-actions button:hover {
    background: var(--btn-surface-hover);
    box-shadow: var(--btn-inset-hover);
    border-color: var(--accent-violet);
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
    border-right: 1px solid var(--bordure);
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
    border: 1px solid var(--bordure);
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
    border-color: var(--accent-violet);
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
    border: 1px solid var(--bordure);
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
        color-mix(in srgb, var(--accent-violet) 25%, var(--fond-controle)) 0%,
        color-mix(in srgb, var(--accent-violet) 12%, var(--fond-controle)) 100%
      );
    color: var(--texte);
    border-color: var(--accent-violet);
  }
  .follower-row:hover {
    background: var(--btn-surface-hover);
    box-shadow: var(--btn-inset-hover);
    border-color: var(--accent-violet);
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
    border: 1px solid var(--bordure);
    cursor: pointer;
    padding: 0.2rem 0.3rem;
    font-size: 0.8rem;
    transition: background 0.15s ease, box-shadow 0.15s ease, border-color 0.15s ease;
  }
  .mini-btn:hover {
    background: var(--btn-surface-hover);
    box-shadow: var(--btn-inset-hover);
    border-color: var(--accent-violet);
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
    background: color-mix(in srgb, var(--accent-violet) 15%, var(--fond-controle));
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
    border: 1px solid var(--bordure);
    padding: 0.4rem 0.8rem;
    cursor: pointer;
    transition: background 0.15s ease, box-shadow 0.15s ease, border-color 0.15s ease;
  }
  .action:hover {
    background: var(--btn-surface-hover);
    box-shadow: var(--btn-inset-hover), var(--glow-violet);
    border-color: var(--accent-violet);
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
    border: 1px solid var(--bordure);
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
    border-color: var(--accent-violet);
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
    scrollbar-color: var(--bordure-active) var(--fond);
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
    background: var(--bordure-active);
    border: 2px solid var(--fond);
  }
  .tab-body::-webkit-scrollbar-thumb:hover,
  .followers-list::-webkit-scrollbar-thumb:hover,
  .clip-picker::-webkit-scrollbar-thumb:hover {
    background: var(--accent-violet);
  }
</style>
