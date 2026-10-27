<script lang="ts">
  // Modale « Interactions » — interactions chat + outils live :
  //   (1) Alertes : config par type (follow/raid/sub/resub/subgift/bits),
  //   (2) Bienvenue : clip de bienvenue (général/file/attribution) +
  //       bandeau premier message (blocs empilés dans le volet détail),
  //   (3) Commandes : commandes chat + soundboard (rail gauche : 2 entrées
  //       de nav, puis le contenu propre à l'outil actif),
  //   (4) Présentation : inviter/présenter un streamer (favoris + diffusion),
  //   (5) Musique : demandes !rs, lecteur, bibliothèque.
  //   + « Position des overlays » : bouton-outil à droite de la barre —
  //     squelette de position partagé des overlays (mini-canvas drag/resize).
  //   Le planning Twitch vit désormais dans PlanningModal.svelte.
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
    welcomeSetUtiliserSquelette,
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
    positionChrono,
    positionSplitpop,
    sauverPositionOverlay,
    sauverPositionChrono,
    sauverPositionSplitpop,
  } from "../stores/positionOverlay";
  import {
    commandesStore,
    chargerCommandes,
    sauverCommande,
    supprimerCommande,
    testerCommande,
    exporterCommandes,
    importerCommandes,
    ecraserCommandes,
  } from "../stores/commandes";
  import {
    cadresChatStore,
    cadresChatPalette,
    chargerCadresChat,
    sauverCadresChat,
    signatureCadresChat,
  } from "../stores/cadresChat";
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
    padImporterSonSeul,
    DISPOSITION_PAD,
    TOUCHES_RESERVEES,
    NOMS_AFFICHAGE,
    NB_PLAGES,
  } from "../stores/padNumerique";
  import {
    presentationEtat,
    chargerPresentation,
    presentationFavoriSauver,
    presentationFavoriSupprimer,
    presentationFavoriSetSon,
    presentationFavoriSetMode,
    presentationRechercher,
    presentationLancer,
    presentationFermer,
    presentationClipJouer,
    presentationClipStopper,
    presentationAudioJouer,
    presentationAudioStopper,
    ecouterSon,
  } from "../stores/presentation";
  import { sceneStore, mettreAJourCadre } from "../stores/scene";
  import { gradientCadre } from "../cadres/registre";
  import { t } from "../stores/langue";
  import { get } from "svelte/store";
  import {
    tauri,
    type WelcomeViewerConfig,
    type WelcomeClipInfo,
    type AlerteTypeConfig,
    type CadreConfig,
    type CommandeConfig,
    type PaletteCouleur,
    type PositionOverlayConfig,
    type ToucheConfig,
    type PresentationSearchResult,
  } from "../tauri";
  import Modal from "./Modal.svelte";
  import MusiqueTab from "./MusiqueTab.svelte";
  import FightTab from "./FightTab.svelte";

  let { onFermer = () => {} }: { onFermer?: () => void } = $props();

  // Onglet actif (« position » n'est pas un onglet de la barre : il s'ouvre
  // via le bouton-outil « Position des overlays » à droite de la barre).
  let tab = $state<
    | "alertes"
    | "bienvenue"
    | "commandes"
    | "presentation"
    | "musique"
    | "fight"
    | "position"
  >("alertes");

  // Section sélectionnée dans les onglets master-detail (liste gauche).
  let secBienvenue = $state<
    "general" | "file" | "attribution" | "bandeau"
  >("general");
  let secCommandes = $state<"commandes" | "cadres" | "pad">("commandes");

  // ===== Présentation streamer (portage « streamers favoris ») =====
  let secPresentation = $state<"recherche" | "favoris" | "diffusion">("favoris");
  let presEtat = $derived($presentationEtat);
  let presFavoris = $derived(presEtat?.favoris ?? []);
  let presCourant = $derived(presEtat?.courant ?? null);
  let presVisible = $derived(presEtat?.visible ?? false);

  // Recherche de chaîne.
  let presQuery = $state("");
  let presResultats = $state<PresentationSearchResult[]>([]);
  let presSearching = $state(false);
  // Erreur des actions (recherche, lancer, sons) — affichée en haut du panneau.
  let presErreur = $state<string | null>(null);
  // user_id en cours de lancement / slug de clip en cours de résolution
  // (spinners sur les boutons — le 1er resolve MP4 peut prendre ~1 s).
  let presLancant = $state<string | null>(null);
  let presClipEnCours = $state<string | null>(null);
  // Login du favori sélectionné dans la liste (panneau détail).
  let presFavSel = $state<string | null>(null);
  let presFavActif = $derived(
    presFavoris.find((f) => f.login === presFavSel) ?? null
  );
  // Pistes audio du favori sélectionné (tuples typés — évite l'union
  // string|null|undefined du destructuring inline dans le markup).
  let presPistes = $derived.by(() => {
    const f = presFavActif;
    if (!f) return [] as { id: string; src: string | null | undefined }[];
    return [
      { id: "voix", src: f.son_voix },
      { id: "musique", src: f.son_musique },
    ];
  });
  // Favori du streamer actuellement présenté (sons à jour — undefined si le
  // streamer présenté n'est pas enregistré en favori).
  let presFavCourant = $derived.by(() => {
    const c = presCourant;
    if (!c) return null;
    return presFavoris.find((f) => f.login === c.login) ?? null;
  });
  // Charge l'état Rust une seule fois à la première ouverture de l'onglet.
  let presCharge = false;

  async function onTabPresentation() {
    tab = "presentation";
    if (!presCharge) {
      presCharge = true;
      await chargerPresentation();
    }
  }

  async function onPresRechercher() {
    const q = presQuery.trim();
    if (!q || presSearching) return;
    presSearching = true;
    presErreur = null;
    try {
      presResultats = await presentationRechercher(q);
    } catch (e) {
      presErreur = String(e);
      presResultats = [];
    } finally {
      presSearching = false;
    }
  }

  async function onPresAjouterFavori(r: PresentationSearchResult) {
    presErreur = null;
    try {
      // thumbnail_url = vignette live (pas l'avatar) — elle est remplacée par
      // le vrai avatar au premier lancer (refresh cache côté Rust).
      await presentationFavoriSauver({
        user_id: r.id,
        login: r.login.toLowerCase(),
        display_name: r.display_name,
        avatar: r.thumbnail_url,
        son_voix: null,
        son_musique: null,
        mode: "theatrical",
      });
      presFavSel = r.login.toLowerCase();
      secPresentation = "favoris";
    } catch (e) {
      presErreur = String(e);
    }
  }

  async function onPresLancer(userId: string, mode?: string) {
    if (presLancant) return;
    presLancant = userId;
    presErreur = null;
    try {
      await presentationLancer(userId, mode);
      secPresentation = "diffusion";
    } catch (e) {
      presErreur = String(e);
    } finally {
      presLancant = null;
    }
  }

  async function onPresFermer() {
    try {
      await presentationFermer();
    } catch (e) {
      presErreur = String(e);
    }
  }

  async function onPresClipJouer(slug: string) {
    if (presClipEnCours) return;
    presClipEnCours = slug;
    presErreur = null;
    try {
      await presentationClipJouer(slug);
    } catch (e) {
      presErreur = String(e);
    } finally {
      presClipEnCours = null;
    }
  }

  async function onPresImporterSon(login: string, piste: string) {
    presErreur = null;
    try {
      const rel = await tauri.importSon();
      if (rel) await presentationFavoriSetSon(login, piste, rel);
    } catch (e) {
      presErreur = String(e);
    }
  }

  async function onPresMode(login: string, mode: string) {
    try {
      await presentationFavoriSetMode(login, mode);
    } catch (e) {
      presErreur = String(e);
    }
  }

  async function onPresSupprimer(login: string) {
    presErreur = null;
    try {
      await presentationFavoriSupprimer(login);
      if (presFavSel === login) presFavSel = null;
    } catch (e) {
      presErreur = String(e);
    }
  }

  /// Nom de fichier d'un chemin medias/… (affichage dans les lignes sons).
  function presBasename(src: string): string {
    return src.split("/").pop() ?? src;
  }

  // ===== Alertes (config par type + overlay) =====
  const NOMS_ALERTE: Record<string, string> = {
    follow: "Follow",
    raid: "Raid",
    sub: "Sub",
    resub: "Resub",
    subgift: "Sub gift",
    bits: "Bits",
    gift: "Gift",
    superchat: "Super Chat",
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

  // Dégradé du cadre actif — affiché dans le picker quand la couleur du
  // texte de commande est « auto » (même règle que les titres).
  let cmdGrad = $derived(gradientCadre($sceneStore.cadreWidget ?? null));

  // ===== Commande cadres (!cadres <couleur> → couleur vive du cadre) =====
  // Commande intégrée (pas une CommandeConfig) : config unique côté Rust.
  let cadresChat = $derived($cadresChatStore);
  let cadresPalette = $derived($cadresChatPalette);

  // Test couleur : applique la signature « !cadres » (vif = couleur
  // choisie, base = noir — signatureCadresChat) sur le cadre actif
  // (commit → la diffusion suit comme pour !cadres) puis restaure les
  // DEUX champs d'origine à l'arrêt. Un seul test à la fois ; restauré
  // aussi à la fermeture.
  let cadreActifScene = $derived(!!$sceneStore.cadreWidget?.actif);
  let cadresTestNom = $state<string | null>(null);
  let cadreAvantTest: Pick<CadreConfig, "couleur" | "couleurFin"> | undefined;

  async function testerCouleurCadres(c: PaletteCouleur) {
    if (cadresTestNom === c.nom) {
      await arreterTestCadres();
      return;
    }
    if (!cadreActifScene) return;
    if (cadresTestNom === null) {
      const cadre = $sceneStore.cadreWidget;
      if (cadre) {
        cadreAvantTest = { couleur: cadre.couleur, couleurFin: cadre.couleurFin };
      }
    }
    cadresTestNom = c.nom;
    await mettreAJourCadre(signatureCadresChat(c.hex));
  }
  async function arreterTestCadres() {
    if (cadresTestNom === null) return;
    cadresTestNom = null;
    await mettreAJourCadre({
      couleur: cadreAvantTest?.couleur,
      couleurFin: cadreAvantTest?.couleurFin,
    });
  }
  // Un test en cours est restauré quand la modale se ferme (cleanup d'effet
  // → destroy) — jamais de couleur « fantôme » laissée sur la scène.
  $effect(() => {
    return () => {
      if (cadresTestNom !== null) void arreterTestCadres();
    };
  });

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
      video_son: true,
      police: "",
      gras: false,
      italique: false,
      souligne: false,
      couleur: undefined,
      reponses_chat: true,
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

  // Import/export pack — même mécanique que les scènes (dossier choisi par
  // l'utilisateur). Les conflits de noms se tranchent un par un dans la
  // barre inline : « écraser » remplace l'existante, « ignorer » la garde.
  let cmdImport = $state<{
    packDir: string;
    file: CommandeConfig[];
    acceptes: CommandeConfig[];
  } | null>(null);

  // Export pack : ligne de saisie inline du nom (pas de prompt() natif).
  let cmdExport = $state(false);
  let cmdExportNom = $state("commandes");

  async function onExporterCommandes() {
    if (await exporterCommandes(cmdExportNom)) cmdExport = false;
  }
  async function onImporterCommandes() {
    const res = await importerCommandes();
    if (res) cmdImport = { packDir: res.packDir, file: res.conflits, acceptes: [] };
  }
  async function trancherConflitCommande(ecraser: boolean) {
    if (!cmdImport) return;
    const packDir = cmdImport.packDir;
    const [courant, ...reste] = cmdImport.file;
    const acceptes = ecraser ? [...cmdImport.acceptes, courant] : cmdImport.acceptes;
    if (reste.length > 0) {
      cmdImport = { packDir, file: reste, acceptes };
      return;
    }
    cmdImport = null;
    if (acceptes.length > 0) await ecraserCommandes(packDir, acceptes);
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

  /// Mode média effectif de la touche :
  /// "aucun" | "audio" | "image" | "video" | "chrono" (chrono 2).
  function modeMediaPad(
    tc: ToucheConfig | null,
  ): "aucun" | "audio" | "image" | "video" | "chrono" {
    if (!tc) return "aucun";
    if (tc.media_type === "audio") return "audio";
    if (tc.media_type === "video") return "video";
    if (tc.media_type === "image") return "image";
    if (tc.media_type === "chrono") return "chrono";
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

  /// Importe un son de départ/fin pour le chrono 2 de la touche sélectionnée
  /// (champs dédiés chrono_son_* — ne touche PAS au champ son du média).
  async function onImporterSonChrono(kind: "depart" | "fin"): Promise<void> {
    if (!padSelCode) return;
    try {
      const rel = await padImporterSonSeul();
      if (rel) {
        majPad(kind === "depart" ? { chrono_son_depart: rel } : { chrono_son_fin: rel });
      }
    } catch (e) {
      alert(get(t)("commun.importRefuse", { erreur: String(e) }));
    }
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

  // Charge les commandes chat + la config cadres (une fois à l'ouverture).
  $effect(() => {
    void chargerCommandes();
    void chargerCadresChat();
  });

  // ===== File d'attente =====
  let etat = $derived($welcomeEtat);
  let actif = $derived(etat?.config_globale_actif ?? false);
  // Mode d'affichage : true = squelette de position (défaut), false =
  // spotlight plein écran (voile noir + carte 85 % + intro avatar 3 s).
  let utiliserSquelette = $derived(etat?.utiliser_squelette ?? true);
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

  // ===== Config overlay : mini-canvas drag/resize (outil « Position des overlays ») =====
  // Le canvas de référence est 1920x1080 (défaut diffusion). Le mini-canvas
  // affiche une version réduite où l'utilisateur drag/resize le squelette
  // de l'overlay. Les coordonnées sont converties en pixels canvas réels.
  //
  // Trois squelettes indépendants : "overlay" (partagé welcome/alertes/
  // commandes/pad — rectangle LIBRE, hauteur auto côté diffusion),
  // "chrono" (squelette DÉDIÉ du chrono 2 — position_chrono.json) et
  // "splitpop" (squelette DÉDIÉ du pop de validation des splits —
  // position_splitpop.json, séparé car le pop arrive à chaque split et
  // pouvait chevaucher les alertes du cadre partagé).
  let overlay = $derived($positionOverlay);
  let overlayChrono = $derived($positionChrono);
  let overlaySplitpop = $derived($positionSplitpop);

  // Dimensions canvas de référence (récupérées depuis la scène si dispo,
  // sinon 1920×1080).
  const REF_W = 1920;
  const REF_H = 1080;

  // Squelette en cours d'édition : "overlay" (partagé) | "chrono" | "splitpop" (dédiés).
  type SqueletteCible = "overlay" | "chrono" | "splitpop";

  // Valeurs locales (édition fluide pendant le drag, commit au pointerup).
  // Initialisées avec les défauts — les $effect ci-dessous sync depuis
  // l'état Rust dès qu'il arrive.
  let ov = $state({ x: 1480, y: 580, w: 320, h: 352 });
  let chr = $state({ x: 760, y: 80, w: 400, h: 120 });
  let spp = $state({ x: 660, y: 60, w: 600, h: 120 });
  let cible = $state<SqueletteCible>("overlay");

  /// Rect courant du squelette ciblé (proxy $state — mutation directe).
  function rectSquelette(c: SqueletteCible) {
    return c === "overlay" ? ov : c === "chrono" ? chr : spp;
  }

  // Sync depuis l'état Rust quand il change (sans écraser pendant le drag
  // du squelette concerné).
  let dragging = $state(false);
  $effect(() => {
    if (overlay && !(dragging && cible === "overlay")) {
      ov.x = overlay.x;
      ov.y = overlay.y;
      ov.w = overlay.largeur;
      ov.h = overlay.hauteur;
    }
  });
  $effect(() => {
    if (overlayChrono && !(dragging && cible === "chrono")) {
      chr.x = overlayChrono.x;
      chr.y = overlayChrono.y;
      chr.w = overlayChrono.largeur;
      chr.h = overlayChrono.hauteur;
    }
  });
  $effect(() => {
    if (overlaySplitpop && !(dragging && cible === "splitpop")) {
      spp.x = overlaySplitpop.x;
      spp.y = overlaySplitpop.y;
      spp.w = overlaySplitpop.largeur;
      spp.h = overlaySplitpop.hauteur;
    }
  });

  // Dimensions réelles du mini-canvas (mesurées via bind:clientWidth).
  // L'échelle utilise la largeur seule — la hauteur du mini-canvas est libre.
  let miniW = $state(300);

  // Échelle : pixels mini-canvas → pixels canvas réels.
  let scale = $derived(miniW / REF_W);

  async function onOverlayCommit(c: SqueletteCible) {
    const r = rectSquelette(c);
    const cfg: PositionOverlayConfig = {
      x: Math.round(r.x),
      y: Math.round(r.y),
      largeur: Math.round(r.w),
      hauteur: Math.round(r.h),
    };
    try {
      if (c === "overlay") {
        await sauverPositionOverlay(cfg);
      } else if (c === "chrono") {
        await sauverPositionChrono(cfg);
      } else {
        await sauverPositionSplitpop(cfg);
      }
    } catch {
    }
  }

  /// Bouton « Tester le rendu » d'un squelette : affiche le VRAI overlay
  /// factice côté diffusion à la position configurée — overlay partagé →
  /// alerte follow de test (même pipeline que les boutons Test de
  /// l'onglet Alertes ; no-op si ce type est désactivé), chrono → chrono
  /// factice de 10 s, splitpop → pop « SPLIT 3 / −1.3 / GAGNÉ » reprenant
  /// les options du 1er widget speedrun.
  async function testerSquelette(c: SqueletteCible) {
    try {
      if (c === "overlay") {
        await tauri.alertesTester("follow");
      } else if (c === "chrono") {
        await tauri.positionChronoTest();
      } else {
        await tauri.positionSplitpopTest();
      }
    } catch {
    }
  }

  // Presets 6 positions (partagés avec l'ancien bloc alertes).
  function presetSquelette(pos: string, c: SqueletteCible) {
    const cw = $sceneStore.canvasW ?? 1920;
    const ch = $sceneStore.canvasH ?? 1080;
    const r = rectSquelette(c);
    const w = r.w;
    const h = r.h;
    const marge = 40;
    let x = r.x;
    let y = r.y;
    if (pos.includes("gauche")) x = marge;
    else if (pos.includes("droite")) x = cw - w - marge;
    else x = Math.round((cw - w) / 2);
    if (pos.startsWith("haut")) y = marge;
    else y = ch - h - marge;
    r.x = x;
    r.y = y;
    void onOverlayCommit(c);
  }

  // ===== Drag / resize des squelettes overlay =====
  type DragMode = "move" | "resize-se" | "resize-sw" | "resize-ne" | "resize-nw" | null;
  let dragMode = $state<DragMode>(null);
  let dragStart = { px: 0, py: 0, x: 0, y: 0, w: 0, h: 0 };

  function onRectDown(e: PointerEvent, c: SqueletteCible) {
    e.stopPropagation();
    e.preventDefault();
    cible = c;
    dragMode = "move";
    dragging = true;
    const r = rectSquelette(c);
    dragStart = { px: e.clientX, py: e.clientY, x: r.x, y: r.y, w: r.w, h: r.h };
    (e.target as HTMLElement).setPointerCapture(e.pointerId);
  }

  function onHandleDown(e: PointerEvent, mode: DragMode, c: SqueletteCible) {
    e.stopPropagation();
    e.preventDefault();
    cible = c;
    dragMode = mode;
    dragging = true;
    const r = rectSquelette(c);
    dragStart = { px: e.clientX, py: e.clientY, x: r.x, y: r.y, w: r.w, h: r.h };
    (e.target as HTMLElement).setPointerCapture(e.pointerId);
  }

  function onPointerMove(e: PointerEvent) {
    if (!dragMode) return;
    const r = rectSquelette(cible);
    const dx = (e.clientX - dragStart.px) / scale;
    const dy = (e.clientY - dragStart.py) / scale;

    if (dragMode === "move") {
      r.x = Math.max(0, Math.min(REF_W - r.w, dragStart.x + dx));
      r.y = Math.max(0, Math.min(REF_H - r.h, dragStart.y + dy));
    } else if (dragMode === "resize-se") {
      // Rectangle libre : largeur et hauteur suivent indépendamment la souris.
      r.w = Math.max(80, Math.min(REF_W - r.x, dragStart.w + dx));
      r.h = Math.max(60, Math.min(REF_H - r.y, dragStart.h + dy));
    } else if (dragMode === "resize-sw") {
      const newW = Math.max(80, dragStart.w - dx);
      r.x = Math.max(0, dragStart.x + (dragStart.w - newW));
      r.w = newW;
      r.h = Math.max(60, Math.min(REF_H - r.y, dragStart.h + dy));
    } else if (dragMode === "resize-ne") {
      const newH = Math.max(60, dragStart.h - dy);
      r.y = Math.max(0, dragStart.y + (dragStart.h - newH));
      r.w = Math.max(80, Math.min(REF_W - r.x, dragStart.w + dx));
      r.h = newH;
    } else if (dragMode === "resize-nw") {
      const newW = Math.max(80, dragStart.w - dx);
      const newH = Math.max(60, dragStart.h - dy);
      r.x = Math.max(0, dragStart.x + (dragStart.w - newW));
      r.y = Math.max(0, dragStart.y + (dragStart.h - newH));
      r.w = newW;
      r.h = newH;
    }
  }

  async function onPointerUp() {
    if (!dragMode) return;
    dragMode = null;
    dragging = false;
    await onOverlayCommit(cible);
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

<Modal title={$t("interactions.titre")} onClose={onFermer} maxWidth="75rem" crochet="var(--crochet-rose)">
  <div class="tabs">
    <button class="tab" class:active={tab === "alertes"} onclick={() => (tab = "alertes")}>
      {$t("interactions.tabAlertes")}
    </button>
    <button class="tab" class:active={tab === "bienvenue"} onclick={() => (tab = "bienvenue")}>
      {$t("interactions.tabBienvenue")}
    </button>
    <button class="tab" class:active={tab === "commandes"} onclick={() => (tab = "commandes")}>
      {$t("interactions.tabCommandes")}
    </button>
    <button
      class="tab"
      class:active={tab === "presentation"}
      onclick={() => { void onTabPresentation(); }}
    >
      {$t("interactions.tabPresentation")}
    </button>
    <button class="tab" class:active={tab === "musique"} onclick={() => (tab = "musique")}>
      {$t("interactions.tabMusique")}
    </button>
    <button class="tab" class:active={tab === "fight"} onclick={() => (tab = "fight")}>
      {$t("interactions.tabFight")}
    </button>
    <button
      class="tab tab-outil"
      class:active={tab === "position"}
      onclick={() => (tab = "position")}
      title={$t("interactions.position.titreOutil")}
    >
      ⌖ {$t("interactions.tabPosition")}
    </button>
  </div>

    {#if tab === "bienvenue"}
      <div class="tab-body">
        <div class="md-layout">
          <!-- Colonne gauche : sections de l'onglet -->
          <div class="md-liste">
            <button
              class="md-select"
              class:selected={secBienvenue === "general"}
              onclick={() => (secBienvenue = "general")}
            >
              <span class="alerte-nom">{$t("commun.general")}</span>
            </button>
            <button
              class="md-select"
              class:selected={secBienvenue === "file"}
              onclick={() => (secBienvenue = "file")}
            >
              <span class="alerte-nom">{$t("interactions.bienvenue.secFile")}</span>
            </button>
            <button
              class="md-select"
              class:selected={secBienvenue === "attribution"}
              onclick={() => (secBienvenue = "attribution")}
            >
              <span class="alerte-nom">{$t("interactions.bienvenue.secAttribution")}</span>
            </button>
            <button
              class="md-select"
              class:selected={secBienvenue === "bandeau"}
              onclick={() => (secBienvenue = "bandeau")}
            >
              <span class="alerte-nom">{$t("interactions.tabBandeau")}</span>
            </button>
          </div>

          <!-- Panneau droit : contenu de la section sélectionnée -->
          <div class="md-detail">
            {#if secBienvenue === "general"}
              <!-- Config globale + overlay -->
              <div class="modal-bloc">
                <div class="modal-bloc-titre">{$t("commun.general")}</div>
                <label class="toggle">
                  <input
                    type="checkbox"
                    checked={actif}
                    onchange={(e) => welcomeSetConfigGlobaleActif(e.currentTarget.checked)}
                  />
                  <span>{$t("interactions.bienvenue.actifs")}</span>
                </label>

                <label class="toggle">
                  <input
                    type="checkbox"
                    checked={utiliserSquelette}
                    onchange={(e) => welcomeSetUtiliserSquelette(e.currentTarget.checked)}
                  />
                  <span>{$t("interactions.bienvenue.utiliserSquelette")}</span>
                </label>
                <p class="hint">{$t("interactions.bienvenue.utiliserSqueletteHint")}</p>

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
              <div class="modal-bloc">
                <div class="modal-bloc-titre">{$t("interactions.bienvenue.fileTitre", { n: queue.length })}</div>

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
              <div class="modal-bloc">
                <div class="modal-bloc-titre">{$t("interactions.bienvenue.attributionTitre")}</div>
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
                              class="action small danger"
                              title={get(t)("interactions.bienvenue.arreterClip")}
                              onclick={onStopperTest}
                            >
                              ⏹ {get(t)("widgetSpeedrun.stop")}
                            </button>
                          {:else}
                            <button
                              class="action small"
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
            {#if secBienvenue === "bandeau"}
              <!-- Bandeau premier message — regroupé en blocs empilés
                   (pas de liste↔détail : chaque bloc est une config courte). -->
              <div class="modal-bloc">
                <div class="modal-bloc-titre">{$t("commun.general")}</div>
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
              <div class="modal-bloc">
                <div class="modal-bloc-titre">{$t("interactions.bandeau.dureeAffichage")}</div>
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
              <div class="modal-bloc">
                <div class="modal-bloc-titre">{$t("interactions.bandeau.position")}</div>
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
              <div class="modal-bloc">
                <div class="modal-bloc-titre">{$t("interactions.bandeau.secTest")}</div>
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
              <div class="modal-bloc">
                <div class="modal-bloc-titre">{$t("interactions.bandeau.apparenceTitre")}</div>
                <p class="hint">
                  {$t("interactions.bandeau.apparenceHint")}
                </p>
              </div>
              <div class="modal-bloc">
                <div class="modal-bloc-titre">{$t("commun.police")}</div>
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

        <div class="md-layout">
          <!-- Colonne gauche : liste des types (nom + ON/OFF) -->
          <div class="md-liste">
            {#each TYPES_ALERTE as ty (ty)}
              {@const c = cfgType(ty)}
              <div class="alerte-item" class:selected={typeActif === ty}>
                <button
                  class="md-select"
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
          <div class="md-detail">
            <div class="modal-bloc">
              <div class="modal-bloc-titre">{$t("commun.media")}</div>
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

            <div class="modal-bloc">
              <div class="modal-bloc-titre">{$t("commun.texte")}</div>
              <label class="alerte-field">
                <span class="field-label">{$t("commun.texte")}</span>
                <input
                  value={cfgSel.texte_template}
                  spellcheck="false"
                  onchange={(e) => majAlerte(typeActif, { texte_template: (e.target as HTMLInputElement).value })}
                />
              </label>
              <p class="hint">
                {$t("interactions.alertes.variablesHint", { vars: "{pseudo} {nbViewers} {nbBits} {niveauSub} {nbMoisCumul} {destinataire} {montant} {objet} {nbObjet}" })}
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

            <div class="modal-bloc">
              <div class="modal-bloc-titre">{$t("commun.comportement")}</div>
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

            <div class="modal-bloc">
              <div class="modal-bloc-titre">{$t("commun.test")}</div>
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
          {secCommandes === "commandes"
            ? $t("interactions.commandes.hintIntro")
            : secCommandes === "cadres"
              ? $t("interactions.cadres.hintIntro")
              : $t("interactions.pad.hintIntro")}
        </p>
        {#if secCommandes === "pad" && padErr}
          <div class="pad-erreur">⚠ {padErr}</div>
        {/if}

        <div class="md-layout">
          <!-- Colonne gauche : nav outil (Commandes chat | Soundboard) puis
               contenu propre à l'outil actif (liste de commandes / blocs pad). -->
          <div class="md-liste">
            <button
              class="md-select"
              class:selected={secCommandes === "commandes"}
              onclick={() => (secCommandes = "commandes")}
            >
              <span class="alerte-nom">{$t("interactions.commandes.navChat")}</span>
            </button>
            <button
              class="md-select"
              class:selected={secCommandes === "cadres"}
              onclick={() => { secCommandes = "cadres"; void chargerCadresChat(); }}
            >
              <span class="alerte-nom">{$t("interactions.cadres.navCadres")}</span>
            </button>
            <button
              class="md-select"
              class:selected={secCommandes === "pad"}
              onclick={() => { secCommandes = "pad"; void chargerPad(); }}
            >
              <span class="alerte-nom">{$t("interactions.tabPad")}</span>
            </button>
            <div class="md-sep"></div>
            {#if secCommandes === "commandes"}
            {#each $commandesStore as c (c.id)}
              <div class="alerte-item" class:selected={cmdSelId === c.id}>
                <button
                  class="md-select"
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
            <div class="cmd-pack">
              <button class="alerte-test" onclick={() => (cmdExport = !cmdExport)}>
                {$t("commun.exporter")}
              </button>
              <button class="alerte-test" onclick={() => void onImporterCommandes()}>
                {$t("commun.importer")}
              </button>
            </div>
            {#if cmdExport}
              <div class="cmd-conflit">
                <label class="alerte-field cmd-export-field">
                  <span class="field-label">{$t("interactions.commandes.nomPackPrompt")}</span>
                  <input
                    bind:value={cmdExportNom}
                    spellcheck="false"
                    onkeydown={(e) => { if (e.key === "Enter") void onExporterCommandes(); }}
                  />
                </label>
                <div class="cmd-conflit-btns">
                  <button class="alerte-test" onclick={() => void onExporterCommandes()}>
                    {$t("commun.valider")}
                  </button>
                  <button class="alerte-test" onclick={() => (cmdExport = false)}>
                    {$t("commun.annuler")}
                  </button>
                </div>
              </div>
            {/if}
            {#if cmdImport}
              <div class="cmd-conflit">
                <span>
                  {$t("interactions.commandes.importConflit", {
                    cmd: "!" + cmdImport.file[0].commande,
                  })}
                </span>
                <div class="cmd-conflit-btns">
                  <button class="alerte-test" onclick={() => void trancherConflitCommande(true)}>
                    {$t("interactions.commandes.ecraser")}
                  </button>
                  <button class="alerte-test" onclick={() => void trancherConflitCommande(false)}>
                    {$t("interactions.commandes.ignorer")}
                  </button>
                </div>
              </div>
            {/if}
            {/if}
            {#if secCommandes === "cadres"}
            <div class="modal-bloc">
              <div class="modal-bloc-titre">{$t("interactions.cadres.activation")}</div>
              <button
                class="alerte-toggle"
                class:on={cadresChat.actif}
                onclick={() => void sauverCadresChat({ actif: !cadresChat.actif })}
                title={cadresChat.actif
                  ? $t("interactions.cadres.desactiver")
                  : $t("interactions.cadres.activer")}
              >{cadresChat.actif ? "ON" : "OFF"}</button>
              <p class="hint">
                {cadresChat.actif
                  ? $t("interactions.cadres.hintActif", { cmd: "!" + (cadresChat.commande || "…") })
                  : $t("interactions.cadres.hintInactif")}
              </p>
            </div>

            <div class="modal-bloc">
              <div class="modal-bloc-titre">{$t("interactions.cadres.nom")}</div>
              <label class="alerte-field">
                <span class="field-label">{$t("interactions.commandes.label")}</span>
                <input
                  value={cadresChat.commande}
                  spellcheck="false"
                  placeholder="cadres"
                  onchange={async (e) => {
                    const el = e.target as HTMLInputElement;
                    const v = el.value.trim().replace(/^!+/, "");
                    // Nom vide ou refusé (collision !commande/musique/fight)
                    // → restaurer le nom courant.
                    if (v === "" || !(await sauverCadresChat({ commande: v }))) {
                      el.value = cadresChat.commande;
                    }
                  }}
                />
              </label>
              <p class="hint">
                {$t("interactions.cadres.hintCommande", { cmd: "!" + (cadresChat.commande || "…") })}
              </p>
            </div>

            <div class="modal-bloc">
              <div class="modal-bloc-titre">{$t("commun.comportement")}</div>
              <label class="toggle">
                <input
                  type="checkbox"
                  checked={cadresChat.reponses_chat !== false}
                  onchange={(e) => void sauverCadresChat({ reponses_chat: e.currentTarget.checked })}
                />
                <span>{$t("interactions.cadres.reponsesChat")}</span>
              </label>
              <p class="hint">
                {$t("interactions.cadres.hintCooldown")}
              </p>
            </div>
            {/if}
            {#if secCommandes === "pad"}
            <div class="modal-bloc">
              <div class="modal-bloc-titre">{$t("interactions.pad.activation")}</div>
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

            <div class="modal-bloc">
              <div class="modal-bloc-titre">{$t("interactions.pad.plages")}</div>
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

            <div class="modal-bloc">
              <div class="modal-bloc-titre">{$t("interactions.pad.touches", { n: padPlage + 1 })}</div>
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
                      class:chrono={mode === "chrono"}
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
            {/if}
          </div>

          <!-- Panneau droit : détail de l'outil actif (commande | touche) -->
          <div class="md-detail">
            {#if secCommandes === "commandes"}
            {#if cmdSel}
              <div class="modal-bloc">
                <div class="modal-bloc-titre">{$t("interactions.commandes.titreBloc")}</div>
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

              <div class="modal-bloc">
                <div class="modal-bloc-titre">{$t("commun.media")}</div>
                <div class="media-modes">
                  <button
                    class="mode-btn"
                    class:active={modeMediaCommande(cmdSel) === "aucun"}
                    onclick={() => majCommande({ media: undefined, media_kind: undefined })}
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
                {#if modeMediaCommande(cmdSel) === "video"}
                  <label class="toggle">
                    <input
                      type="checkbox"
                      checked={cmdSel.video_son !== false}
                      onchange={(e) => majCommande({ video_son: e.currentTarget.checked })}
                    />
                    <span>{$t("interactions.commandes.sonVideo")}</span>
                  </label>
                {/if}
                <!-- Son optionnel dans TOUS les modes : joué par-dessus
                     (la vidéo garde son audio — mix). -->
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
                <p class="hint">
                  {$t("interactions.commandes.hintMedia")}
                </p>
              </div>

              <div class="modal-bloc">
                <div class="modal-bloc-titre">{$t("commun.texte")}</div>
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
                <label class="alerte-field">
                  <span class="field-label">{$t("commun.police")}</span>
                  <select
                    class="police-select"
                    value={cmdSel.police ?? ""}
                    onchange={(e) => majCommande({ police: e.currentTarget.value })}
                  >
                    <option value="">{$t("interactions.bandeau.policeSysteme")}</option>
                    {#each POLICES_TITRE as p (p.id)}
                      <option value={p.id} style="font-family: '{p.id}', sans-serif;">{p.nom}</option>
                    {/each}
                  </select>
                </label>
                <div class="alerte-field">
                  <span class="field-label">{$t("titreForm.style")}</span>
                  <div class="style-boutons">
                    <button
                      class="style-btn"
                      class:active={cmdSel.gras}
                      title={$t("titreForm.gras")}
                      onclick={() => majCommande({ gras: !cmdSel.gras })}
                    ><b>G</b></button>
                    <button
                      class="style-btn"
                      class:active={cmdSel.italique}
                      title={$t("titreForm.italique")}
                      onclick={() => majCommande({ italique: !cmdSel.italique })}
                    ><i>I</i></button>
                    <button
                      class="style-btn"
                      class:active={cmdSel.souligne}
                      title={$t("titreForm.souligne")}
                      onclick={() => majCommande({ souligne: !cmdSel.souligne })}
                    ><u>U</u></button>
                  </div>
                </div>
                <div class="alerte-field">
                  <span class="field-label">{$t("titreForm.couleurTexte")}</span>
                  <div class="cmd-couleur-row">
                    <input
                      class="couleur-input"
                      type="color"
                      title={$t("titreForm.couleurDebutTitle")}
                      value={cmdSel.couleur ?? cmdGrad.from}
                      disabled={cmdSel.couleur == null}
                      oninput={(e) => majCommande({ couleur: e.currentTarget.value })}
                    />
                    {#if cmdSel.couleur != null}
                      <button
                        class="auto-btn"
                        title={$t("titreForm.autoCadreTitle")}
                        onclick={() => majCommande({ couleur: undefined })}
                      >{$t("titreForm.autoCadre")}</button>
                    {:else}
                      <button
                        class="auto-btn"
                        title={$t("titreForm.personnaliserTitle")}
                        onclick={() => majCommande({ couleur: cmdGrad.from })}
                      >{$t("titreForm.personnaliser")}</button>
                      <span class="auto-tag">{$t("titreForm.autoTag")}</span>
                    {/if}
                  </div>
                </div>
              </div>

              <div class="modal-bloc">
                <div class="modal-bloc-titre">{$t("commun.comportement")}</div>
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
                <label class="toggle">
                  <input
                    type="checkbox"
                    checked={cmdSel.reponses_chat !== false}
                    onchange={(e) => majCommande({ reponses_chat: e.currentTarget.checked })}
                  />
                  <span>{$t("interactions.commandes.reponsesChat")}</span>
                </label>
              </div>

              <div class="modal-bloc">
                <div class="modal-bloc-titre">{$t("interactions.testSuppression")}</div>
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
              <div class="modal-bloc">
                <p class="hint">{$t("interactions.commandes.aucuneSelection")}</p>
              </div>
          {/if}
            {/if}
            {#if secCommandes === "pad"}
            {#if padSelCode && padSel}
              <div class="modal-bloc">
                <div class="modal-bloc-titre">{$t("interactions.pad.toucheTitre", { touche: NOMS_AFFICHAGE[padSelCode] ?? padSelCode, plage: padPlage + 1 })}</div>
                <p class="hint">
                  {$t("interactions.pad.hintTouche")}
                </p>
              </div>

              <div class="modal-bloc">
                <div class="modal-bloc-titre">{$t("commun.media")}</div>
                <div class="media-modes pad-modes">
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
                  <button
                    class="mode-btn"
                    class:active={modeMediaPad(padSel) === "chrono"}
                    onclick={() => majPad({ media_type: "chrono", media: undefined, media_kind: undefined, son: undefined })}
                  >{$t("commun.chrono")}</button>
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

                {#if modeMediaPad(padSel) === "chrono"}
                  <label class="alerte-field">
                    <span class="field-label">{$t("interactions.pad.chronoDuree")}</span>
                    <input
                      type="number"
                      min="1"
                      max="3600"
                      value={padSel.chrono_duree_s}
                      onchange={(e) => majPad({ chrono_duree_s: Math.max(1, Math.round(+(e.target as HTMLInputElement).value)) })}
                    />
                  </label>
                  <label class="alerte-field">
                    <span class="field-label">{$t("interactions.pad.chronoSonDepart")}</span>
                    {#if padSel.chrono_son_depart}
                      <span class="alerte-son-nom">{padSel.chrono_son_depart.split("/").pop()}</span>
                      <button
                        class="alerte-toggle"
                        onclick={() => majPad({ chrono_son_depart: undefined })}
                        title={$t("commun.retirerSon")}
                      >✕</button>
                    {:else}
                      <button class="alerte-test" onclick={() => void onImporterSonChrono("depart")}>
                        {$t("commun.importerSon")}
                      </button>
                    {/if}
                  </label>
                  <label class="alerte-field">
                    <span class="field-label">{$t("interactions.pad.chronoSonFin")}</span>
                    {#if padSel.chrono_son_fin}
                      <span class="alerte-son-nom">{padSel.chrono_son_fin.split("/").pop()}</span>
                      <button
                        class="alerte-toggle"
                        onclick={() => majPad({ chrono_son_fin: undefined })}
                        title={$t("commun.retirerSon")}
                      >✕</button>
                    {:else}
                      <button class="alerte-test" onclick={() => void onImporterSonChrono("fin")}>
                        {$t("commun.importerSon")}
                      </button>
                    {/if}
                  </label>
                  <p class="hint">
                    {$t("interactions.pad.hintChrono")}
                  </p>
                {/if}

                {#if modeMediaPad(padSel) !== "chrono"}
                  <p class="hint">
                    {$t("interactions.pad.hintMedia")}
                  </p>
                {/if}
              </div>

              <div class="modal-bloc">
                <div class="modal-bloc-titre">{$t("commun.comportement")}</div>
                <div class="alerte-2col">
                  {#if modeMediaPad(padSel) !== "chrono"}
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
                  {/if}
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
                  <div class="pad-checks">
                    <label class="pad-check">
                      <input
                        type="checkbox"
                        checked={padSel.bounce}
                        onchange={(e) => majPad({ bounce: (e.target as HTMLInputElement).checked })}
                      />
                      <span>Bounce</span>
                    </label>
                    <label class="pad-check">
                      <input
                        type="checkbox"
                        checked={padSel.zoom}
                        onchange={(e) => majPad({ zoom: (e.target as HTMLInputElement).checked })}
                      />
                      <span>Zoom</span>
                    </label>
                    <label class="pad-check">
                      <input
                        type="checkbox"
                        checked={padSel.plein_ecran}
                        onchange={(e) => majPad({ plein_ecran: (e.target as HTMLInputElement).checked })}
                      />
                      <span>{$t("interactions.pad.pleinEcran")}</span>
                    </label>
                    {#if padSel.plein_ecran}
                      <label class="pad-check">
                        <input
                          type="checkbox"
                          checked={padSel.transparence}
                          onchange={(e) => majPad({ transparence: (e.target as HTMLInputElement).checked })}
                        />
                        <span>{$t("interactions.pad.transparence")}</span>
                      </label>
                    {/if}
                  </div>
                  {#if padSel.plein_ecran && padSel.transparence}
                    <label class="alerte-field">
                      <span class="field-label">
                        {$t("interactions.pad.opacite", { n: Math.round(padSel.transparence_opacite * 100) })}
                      </span>
                      <input
                        class="range"
                        type="range"
                        min="0"
                        max="100"
                        step="5"
                        value={Math.round(padSel.transparence_opacite * 100)}
                        oninput={(e) => majPad({ transparence_opacite: +(e.target as HTMLInputElement).value / 100 })}
                      />
                    </label>
                  {/if}
                  <p class="hint">
                    {$t("interactions.pad.hintPleinEcran")}
                  </p>
                {/if}
              </div>

              <div class="modal-bloc">
                <div class="modal-bloc-titre">{$t("interactions.testSuppression")}</div>
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
              <div class="modal-bloc">
                <p class="hint">
                  {$t("interactions.pad.hintSelection")}
                </p>
              </div>
            {/if}
            {/if}
            {#if secCommandes === "cadres"}
              <div class="modal-bloc">
                <div class="modal-bloc-titre">
                  {$t("interactions.cadres.paletteTitre", { n: cadresPalette.length })}
                </div>
                <p class="hint">
                  {$t("interactions.cadres.hintPalette", { cmd: "!" + (cadresChat.commande || "…") })}
                </p>
                <div class="cadres-palette">
                  {#each cadresPalette as c (c.nom)}
                    <div
                      class="cadres-couleur"
                      class:en-test={cadresTestNom === c.nom}
                      title="{c.nom} — {c.hex}"
                    >
                      <span class="cadres-chip" style:background={c.hex}></span>
                      <span class="cadres-nom">{c.nom}</span>
                      <button
                        class="cadres-test"
                        class:on={cadresTestNom === c.nom}
                        disabled={!cadreActifScene}
                        title={!cadreActifScene
                          ? $t("interactions.cadres.pasCadre")
                          : cadresTestNom === c.nom
                            ? $t("interactions.cadres.stopTest")
                            : $t("interactions.cadres.tester")}
                        onclick={() => void testerCouleurCadres(c)}
                      >{cadresTestNom === c.nom ? "■" : "▶"}</button>
                    </div>
                  {/each}
                </div>
              </div>
            {/if}
          </div>
        </div>
      </div>
    {/if}

    {#if tab === "presentation"}
      <div class="tab-body">
        <div class="md-layout">
          <!-- Colonne gauche : sections de l'onglet -->
          <div class="md-liste">
            <button
              class="md-select"
              class:selected={secPresentation === "recherche"}
              onclick={() => (secPresentation = "recherche")}
            >
              <span class="alerte-nom">{$t("interactions.presentation.secRecherche")}</span>
            </button>
            <button
              class="md-select"
              class:selected={secPresentation === "favoris"}
              onclick={() => (secPresentation = "favoris")}
            >
              <span class="alerte-nom">{$t("interactions.presentation.secFavoris")}</span>
            </button>
            <button
              class="md-select"
              class:selected={secPresentation === "diffusion"}
              onclick={() => (secPresentation = "diffusion")}
              disabled={!presVisible}
            >
              <span class="alerte-nom">{$t("interactions.presentation.secDiffusion")}</span>
            </button>
          </div>

          <!-- Panneau droit : contenu de la section sélectionnée -->
          <div class="md-detail">
            {#if presErreur}
              <p class="hint pres-erreur">{presErreur}</p>
            {/if}

            {#if secPresentation === "recherche"}
              <!-- Recherche de chaîne Twitch -->
              <div class="modal-bloc">
                <div class="modal-bloc-titre">{$t("interactions.presentation.rechercheTitre")}</div>
                <div class="pres-search-row">
                  <input
                    class="search-input"
                    type="text"
                    placeholder={$t("interactions.presentation.rechercherPlaceholder")}
                    bind:value={presQuery}
                    onkeydown={(e) => { if (e.key === "Enter") void onPresRechercher(); }}
                  />
                  <button
                    class="action"
                    onclick={onPresRechercher}
                    disabled={presSearching || !presQuery.trim()}
                  >
                    {presSearching ? "…" : $t("commun.rechercher")}
                  </button>
                </div>
                <p class="hint">{$t("interactions.presentation.rechercheHint")}</p>

                {#if presResultats.length > 0}
                  <ul class="queue-list">
                    {#each presResultats as r (r.id)}
                      <li class="item">
                        <img src={r.thumbnail_url} alt="" class="thumb" />
                        <div class="item-info">
                          <div class="item-pseudo">
                            {r.display_name}
                            {#if r.is_live}
                              <span class="pres-live-dot"></span>
                            {/if}
                          </div>
                          <div class="item-clip">@{r.login}</div>
                        </div>
                        <div class="item-actions">
                          <button
                            class="mini-btn"
                            title={$t("interactions.presentation.ajouterFavori")}
                            onclick={() => onPresAjouterFavori(r)}
                          >
                            ＋
                          </button>
                          <button
                            class="mini-btn"
                            title={$t("interactions.presentation.presenter")}
                            onclick={() => onPresLancer(r.id)}
                            disabled={presLancant !== null}
                          >
                            {presLancant === r.id ? "…" : "🎭"}
                          </button>
                        </div>
                      </li>
                    {/each}
                  </ul>
                {/if}
              </div>
            {/if}

            {#if secPresentation === "favoris"}
              <!-- Liste des favoris + détail du favori sélectionné -->
              <div class="attribution-body">
                <div class="followers-list">
                  <h3>{$t("interactions.presentation.favorisTitre", { n: presFavoris.length })}</h3>
                  {#if presFavoris.length === 0}
                    <p class="empty">{$t("interactions.presentation.aucunFavori")}</p>
                  {:else}
                    <ul>
                      {#each presFavoris as f (f.login)}
                        <li>
                          <button
                            class="follower-row"
                            class:selected={presFavSel === f.login}
                            onclick={() => (presFavSel = f.login)}
                          >
                            {#if f.avatar}
                              <img src={f.avatar} alt="" class="avatar" />
                            {/if}
                            <span class="login">{f.display_name}</span>
                          </button>
                        </li>
                      {/each}
                    </ul>
                  {/if}
                </div>

                {#if presFavActif}
                  <div class="clip-picker">
                    <h3>{presFavActif.display_name} <span class="pres-at">@{presFavActif.login}</span></h3>

                    <!-- Mode d'affichage (persisté — bascule à chaud si ce
                         streamer est actuellement présenté). -->
                    <div class="field-label">{$t("interactions.presentation.modeTitre")}</div>
                    <div class="media-modes pres-mode-2">
                      <button
                        class="mode-btn"
                        class:active={presFavActif.mode === "theatrical"}
                        onclick={() => onPresMode(presFavActif.login, "theatrical")}
                      >
                        {$t("interactions.presentation.modeTheatral")}
                      </button>
                      <button
                        class="mode-btn"
                        class:active={presFavActif.mode === "card"}
                        onclick={() => onPresMode(presFavActif.login, "card")}
                      >
                        {$t("interactions.presentation.modeCarte")}
                      </button>
                    </div>

                    <!-- Pistes audio (voix + musique, fichiers medias/). -->
                    <div class="field-label">{$t("interactions.presentation.sonsTitre")}</div>
                    {#each presPistes as p (p.id)}
                      <div class="pres-son-row">
                        <span class="pres-son-label">
                          {p.id === "voix" ? $t("interactions.presentation.sonVoix") : $t("interactions.presentation.sonMusique")}
                        </span>
                        {#if p.src}
                          <span class="pres-son-nom" title={p.src}>{presBasename(p.src)}</span>
                          <button
                            class="mini-btn"
                            title={$t("interactions.presentation.ecouter")}
                            onclick={() => ecouterSon(p.src as string)}
                          >
                            ▶
                          </button>
                          <button
                            class="mini-btn"
                            title={$t("interactions.presentation.retirerSon")}
                            onclick={() => presentationFavoriSetSon(presFavActif.login, p.id, null)}
                          >
                            ✕
                          </button>
                        {:else}
                          <button
                            class="mini-btn"
                            onclick={() => onPresImporterSon(presFavActif.login, p.id)}
                          >
                            ＋ {$t("commun.importer")}
                          </button>
                        {/if}
                      </div>
                    {/each}
                    <p class="hint">{$t("interactions.presentation.sonsHint")}</p>

                    <div class="actions-bar">
                      <button
                        class="action"
                        onclick={() => onPresLancer(presFavActif.user_id)}
                        disabled={presLancant !== null}
                      >
                        {presLancant === presFavActif.user_id ? "…" : `🎭 ${$t("interactions.presentation.presenter")}`}
                      </button>
                      <button
                        class="mini-btn"
                        title={$t("interactions.presentation.retirerFavori")}
                        onclick={() => onPresSupprimer(presFavActif.login)}
                      >
                        ✕ {$t("commun.retirer")}
                      </button>
                    </div>
                  </div>
                {:else if presFavoris.length > 0}
                  <div class="clip-picker">
                    <p class="empty">{$t("interactions.presentation.selectionnezFavori")}</p>
                  </div>
                {/if}
              </div>
            {/if}

            {#if secPresentation === "diffusion"}
              <!-- Contrôles de la présentation en cours -->
              <div class="modal-bloc">
                {#if presCourant}
                  <div class="modal-bloc-titre">{$t("interactions.presentation.enCoursTitre", { nom: presCourant.display_name })}</div>

                  <div class="actions-bar">
                    <button class="action" onclick={onPresFermer}>
                      {$t("interactions.presentation.fermer")}
                    </button>
                    <button class="action" onclick={() => presentationClipStopper()}>
                      {$t("interactions.presentation.stopClip")}
                    </button>
                  </div>

                  <!-- Bascule de mode à chaud. -->
                  <div class="field-label">{$t("interactions.presentation.modeTitre")}</div>
                  <div class="media-modes pres-mode-2">
                    <button
                      class="mode-btn"
                      class:active={presCourant.mode === "theatrical"}
                      onclick={() => onPresMode(presCourant.login, "theatrical")}
                    >
                      {$t("interactions.presentation.modeTheatral")}
                    </button>
                    <button
                      class="mode-btn"
                      class:active={presCourant.mode === "card"}
                      onclick={() => onPresMode(presCourant.login, "card")}
                    >
                      {$t("interactions.presentation.modeCarte")}
                    </button>
                  </div>

                  <!-- Pistes audio du favori (si enregistré + configuré). -->
                  {#if presFavCourant && (presFavCourant.son_voix || presFavCourant.son_musique)}
                    <div class="field-label">{$t("interactions.presentation.sonsTitre")}</div>
                    <div class="actions-bar">
                      {#if presFavCourant.son_voix}
                        <button
                          class="action small"
                          onclick={() =>
                            presentationAudioJouer("voix", presFavCourant.son_voix as string)
                          }
                        >
                          ▶ {$t("interactions.presentation.sonVoix")}
                        </button>
                      {/if}
                      {#if presFavCourant.son_musique}
                        <button
                          class="action small"
                          onclick={() =>
                            presentationAudioJouer("musique", presFavCourant.son_musique as string)
                          }
                        >
                          ▶ {$t("interactions.presentation.sonMusique")}
                        </button>
                      {/if}
                      <button
                        class="action small danger"
                        onclick={() => presentationAudioStopper("toutes")}
                      >
                        ⏹ {$t("interactions.presentation.stopSons")}
                      </button>
                    </div>
                  {:else}
                    <p class="hint">{$t("interactions.presentation.sonsFavoriHint")}</p>
                  {/if}

                  <!-- Clips récents : clic = lecture sur le stream. -->
                  <div class="field-label">{$t("interactions.presentation.clipsTitre", { n: presCourant.clips.length })}</div>
                  {#if presCourant.clips.length === 0}
                    <p class="empty">{$t("interactions.presentation.aucunClip")}</p>
                  {:else}
                    <ul class="clips-list">
                      {#each presCourant.clips as clip (clip.id)}
                        <li>
                          <div class="clip-row">
                            <img src={clip.thumbnail_url} alt="" class="thumb" />
                            <div class="clip-info">
                              <div class="clip-title">{clip.title}</div>
                              <div class="clip-meta">{Math.round(clip.duration)}s · {clip.created_at.slice(0, 10)}</div>
                            </div>
                          </div>
                          <div class="clip-actions">
                            <button
                              class="action small"
                              title={$t("interactions.presentation.jouerClip")}
                              onclick={() => onPresClipJouer(clip.id)}
                              disabled={presClipEnCours !== null}
                            >
                              {presClipEnCours === clip.id ? "…" : $t("interactions.presentation.jouer")}
                            </button>
                          </div>
                        </li>
                      {/each}
                    </ul>
                  {/if}
                {:else}
                  <p class="empty">{$t("interactions.presentation.aucunePresentation")}</p>
                {/if}
              </div>
            {/if}
          </div>
        </div>
      </div>
    {/if}

    {#if tab === "musique"}
      <div class="tab-body">
        <MusiqueTab />
      </div>
    {/if}

    {#if tab === "fight"}
      <div class="tab-body">
        <FightTab />
      </div>
    {/if}

    {#if tab === "position"}
      <div class="tab-body">
        <p class="hint">
          {$t("interactions.position.hintIntro")}
        </p>

        <div class="modal-bloc">
          <div class="modal-bloc-titre">{$t("interactions.tabPosition")}</div>
          <div class="alerte-presets">
            {#each Object.entries(PRESETS_OVERLAY) as [pos, label] (pos)}
              <button class="alerte-test" title={get(t)("interactions.position.pos." + pos)} onclick={() => presetSquelette(pos, "overlay")}>{label}</button>
            {/each}
            <button class="alerte-test" onclick={() => void testerSquelette("overlay")}>{$t("interactions.position.tester")}</button>
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
              style="left: {ov.x * scale}px; top: {ov.y * scale}px; width: {ov.w * scale}px; height: {ov.h * scale}px;"
              onpointerdown={(e) => onRectDown(e, "overlay")}
              role="presentation"
            >
              <span class="skeleton-label">{$t("interactions.position.overlay")}</span>
              <div class="sk-handle nw" onpointerdown={(e) => onHandleDown(e, "resize-nw", "overlay")} role="presentation"></div>
              <div class="sk-handle ne" onpointerdown={(e) => onHandleDown(e, "resize-ne", "overlay")} role="presentation"></div>
              <div class="sk-handle sw" onpointerdown={(e) => onHandleDown(e, "resize-sw", "overlay")} role="presentation"></div>
              <div class="sk-handle se" onpointerdown={(e) => onHandleDown(e, "resize-se", "overlay")} role="presentation"></div>
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
                value={Math.round(ov.x)}
                onchange={(e) => { ov.x = +(e.target as HTMLInputElement).value; void onOverlayCommit("overlay"); }}
              />
            </label>
            <label class="alerte-field">
              <span class="field-label">Y</span>
              <input
                type="number"
                value={Math.round(ov.y)}
                onchange={(e) => { ov.y = +(e.target as HTMLInputElement).value; void onOverlayCommit("overlay"); }}
              />
            </label>
            <label class="alerte-field">
              <span class="field-label">{$t("commun.largeur")}</span>
              <input
                type="number"
                min="80"
                max="1920"
                value={Math.round(ov.w)}
                onchange={(e) => { ov.w = Math.max(80, +(e.target as HTMLInputElement).value); void onOverlayCommit("overlay"); }}
              />
            </label>
            <label class="alerte-field">
              <span class="field-label">{$t("interactions.position.hauteurRef")}</span>
              <input
                type="number"
                min="60"
                max="1080"
                value={Math.round(ov.h)}
                onchange={(e) => { ov.h = Math.max(60, +(e.target as HTMLInputElement).value); void onOverlayCommit("overlay"); }}
              />
            </label>
          </div>
        </div>

        <div class="modal-bloc">
          <div class="modal-bloc-titre">{$t("interactions.position.titreChrono")}</div>
          <div class="alerte-presets">
            {#each Object.entries(PRESETS_OVERLAY) as [pos, label] (pos)}
              <button class="alerte-test" title={get(t)("interactions.position.pos." + pos)} onclick={() => presetSquelette(pos, "chrono")}>{label}</button>
            {/each}
            <button class="alerte-test" onclick={() => void testerSquelette("chrono")}>{$t("interactions.position.tester")}</button>
          </div>
          <div
            class="mini-canvas"
            bind:clientWidth={miniW}
            onpointermove={onPointerMove}
            onpointerup={onPointerUp}
            role="presentation"
          >
            <div
              class="overlay-skeleton squelette-chrono"
              style="left: {chr.x * scale}px; top: {chr.y * scale}px; width: {chr.w * scale}px; height: {chr.h * scale}px;"
              onpointerdown={(e) => onRectDown(e, "chrono")}
              role="presentation"
            >
              <span class="skeleton-label">{$t("interactions.position.chrono")}</span>
              <div class="sk-handle nw" onpointerdown={(e) => onHandleDown(e, "resize-nw", "chrono")} role="presentation"></div>
              <div class="sk-handle ne" onpointerdown={(e) => onHandleDown(e, "resize-ne", "chrono")} role="presentation"></div>
              <div class="sk-handle sw" onpointerdown={(e) => onHandleDown(e, "resize-sw", "chrono")} role="presentation"></div>
              <div class="sk-handle se" onpointerdown={(e) => onHandleDown(e, "resize-se", "chrono")} role="presentation"></div>
            </div>
          </div>
          <p class="hint">
            {$t("interactions.position.hintChrono")}
          </p>
          <div class="alerte-4col">
            <label class="alerte-field">
              <span class="field-label">X</span>
              <input
                type="number"
                value={Math.round(chr.x)}
                onchange={(e) => { chr.x = +(e.target as HTMLInputElement).value; void onOverlayCommit("chrono"); }}
              />
            </label>
            <label class="alerte-field">
              <span class="field-label">Y</span>
              <input
                type="number"
                value={Math.round(chr.y)}
                onchange={(e) => { chr.y = +(e.target as HTMLInputElement).value; void onOverlayCommit("chrono"); }}
              />
            </label>
            <label class="alerte-field">
              <span class="field-label">{$t("commun.largeur")}</span>
              <input
                type="number"
                min="80"
                max="1920"
                value={Math.round(chr.w)}
                onchange={(e) => { chr.w = Math.max(80, +(e.target as HTMLInputElement).value); void onOverlayCommit("chrono"); }}
              />
            </label>
            <label class="alerte-field">
              <span class="field-label">{$t("interactions.position.hauteurRef")}</span>
              <input
                type="number"
                min="60"
                max="1080"
                value={Math.round(chr.h)}
                onchange={(e) => { chr.h = Math.max(60, +(e.target as HTMLInputElement).value); void onOverlayCommit("chrono"); }}
              />
            </label>
          </div>
        </div>

        <div class="modal-bloc">
          <div class="modal-bloc-titre">{$t("interactions.position.titreSplitpop")}</div>
          <div class="alerte-presets">
            {#each Object.entries(PRESETS_OVERLAY) as [pos, label] (pos)}
              <button class="alerte-test" title={get(t)("interactions.position.pos." + pos)} onclick={() => presetSquelette(pos, "splitpop")}>{label}</button>
            {/each}
            <button class="alerte-test" onclick={() => void testerSquelette("splitpop")}>{$t("interactions.position.tester")}</button>
          </div>
          <div
            class="mini-canvas"
            bind:clientWidth={miniW}
            onpointermove={onPointerMove}
            onpointerup={onPointerUp}
            role="presentation"
          >
            <div
              class="overlay-skeleton squelette-splitpop"
              style="left: {spp.x * scale}px; top: {spp.y * scale}px; width: {spp.w * scale}px; height: {spp.h * scale}px;"
              onpointerdown={(e) => onRectDown(e, "splitpop")}
              role="presentation"
            >
              <span class="skeleton-label">{$t("interactions.position.splitpop")}</span>
              <div class="sk-handle nw" onpointerdown={(e) => onHandleDown(e, "resize-nw", "splitpop")} role="presentation"></div>
              <div class="sk-handle ne" onpointerdown={(e) => onHandleDown(e, "resize-ne", "splitpop")} role="presentation"></div>
              <div class="sk-handle sw" onpointerdown={(e) => onHandleDown(e, "resize-sw", "splitpop")} role="presentation"></div>
              <div class="sk-handle se" onpointerdown={(e) => onHandleDown(e, "resize-se", "splitpop")} role="presentation"></div>
            </div>
          </div>
          <p class="hint">
            {$t("interactions.position.hintSplitpop")}
          </p>
          <div class="alerte-4col">
            <label class="alerte-field">
              <span class="field-label">X</span>
              <input
                type="number"
                value={Math.round(spp.x)}
                onchange={(e) => { spp.x = +(e.target as HTMLInputElement).value; void onOverlayCommit("splitpop"); }}
              />
            </label>
            <label class="alerte-field">
              <span class="field-label">Y</span>
              <input
                type="number"
                value={Math.round(spp.y)}
                onchange={(e) => { spp.y = +(e.target as HTMLInputElement).value; void onOverlayCommit("splitpop"); }}
              />
            </label>
            <label class="alerte-field">
              <span class="field-label">{$t("commun.largeur")}</span>
              <input
                type="number"
                min="80"
                max="1920"
                value={Math.round(spp.w)}
                onchange={(e) => { spp.w = Math.max(80, +(e.target as HTMLInputElement).value); void onOverlayCommit("splitpop"); }}
              />
            </label>
            <label class="alerte-field">
              <span class="field-label">{$t("interactions.position.hauteurRef")}</span>
              <input
                type="number"
                min="60"
                max="1080"
                value={Math.round(spp.h)}
                onchange={(e) => { spp.h = Math.max(60, +(e.target as HTMLInputElement).value); void onOverlayCommit("splitpop"); }}
              />
            </label>
          </div>
        </div>
      </div>
    {/if}
</Modal>

<style>
  /* ===== Master-detail (liste gauche + détail droite) : recette partagée
     .md-* (app.css) — .alerte-item reste le wrapper ligne [select+toggle]. */
  .alerte-item {
    display: flex;
    align-items: center;
    gap: 0.25rem;
  }
  .cmd-ajouter {
    margin-top: 0.25rem;
  }
  .cmd-pack {
    display: flex;
    gap: 0.25rem;
    margin-top: 0.25rem;
  }
  .cmd-pack .alerte-test {
    flex: 1;
  }
  /* Barre de résolution des conflits d'import (un nom à la fois). */
  .cmd-conflit {
    margin-top: 0.4rem;
    padding: 0.5rem 0.6rem;
    background: color-mix(in srgb, var(--message-warn-color, #e6a23c) 15%, transparent);
    border: 1px solid color-mix(in srgb, var(--message-warn-color, #e6a23c) 60%, transparent);
    border-radius: var(--rayon);
    font-size: 0.8rem;
  }
  .cmd-conflit-btns {
    display: flex;
    gap: 0.25rem;
    margin-top: 0.4rem;
  }
  /* Le label du nom de pack réutilise .alerte-field : on neutralise juste
     sa marge basse dans la barre inline. */
  .cmd-export-field {
    margin-bottom: 0;
  }
  /* Style du texte de commande (G/I/S + couleur) — recettes reprises de
     TitreEditForm (boutons style, picker couleur, bouton auto/perso). */
  .style-boutons {
    display: flex;
    gap: 0.4rem;
  }
  .style-btn {
    flex: 0 0 2.2rem;
    height: 2rem;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--btn-surface);
    box-shadow: var(--btn-inset);
    color: var(--texte);
    border: 1px solid var(--bordure-ui);
    font: inherit;
    font-size: 0.95rem;
    cursor: pointer;
    opacity: 0.7;
    transition: background 0.15s ease, box-shadow 0.15s ease, border-color 0.15s ease, opacity 0.15s ease;
  }
  .style-btn:hover {
    background: var(--btn-surface-hover);
    box-shadow: var(--btn-inset-hover);
    border-color: var(--accent-ui);
    opacity: 1;
  }
  .style-btn.active {
    background:
      linear-gradient(
        180deg,
        color-mix(in srgb, var(--accent-ui) 25%, var(--fond-controle)) 0%,
        color-mix(in srgb, var(--accent-ui) 12%, var(--fond-controle)) 100%
      );
    color: var(--texte);
    border-color: var(--accent-ui);
    opacity: 1;
  }
  .cmd-couleur-row {
    display: flex;
    align-items: center;
    gap: 0.6rem;
  }
  /* NB : .alerte-field input (width:100%) gagnerait en spécificité sur un
     sélecteur classe seule — d'où le préfixe .cmd-couleur-row ici. */
  .cmd-couleur-row .couleur-input {
    width: 1.75rem;
    height: 1.5rem;
    border: 1px solid var(--bordure-ui);
    background: transparent;
    cursor: pointer;
    flex-shrink: 0;
    padding: 0;
  }
  .cmd-couleur-row .couleur-input:disabled {
    opacity: 0.45;
    cursor: default;
  }
  .auto-btn {
    padding: 0.25rem 0.6rem;
    background: var(--btn-surface);
    box-shadow: var(--btn-inset);
    color: var(--texte);
    border: 1px solid var(--bordure-ui);
    font: inherit;
    font-size: 0.78rem;
    cursor: pointer;
    flex-shrink: 0;
    opacity: 0.85;
    transition: background 0.15s ease, border-color 0.15s ease, opacity 0.15s ease;
  }
  .auto-btn:hover {
    background: var(--btn-surface-hover);
    border-color: var(--accent-ui);
    opacity: 1;
  }
  .auto-tag {
    font-size: 0.75rem;
    font-style: italic;
    opacity: 0.55;
  }
  /* Segmented control mode média (Aucun | Image | Vidéo) */
  .media-modes {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 0.25rem;
    margin-bottom: 0.4rem;
  }
  /* Pad : 5 modes (Aucun | Audio | Image | Vidéo | Chrono) */
  .media-modes.pad-modes {
    grid-template-columns: repeat(5, 1fr);
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
  .pad-touche.chrono { background: color-mix(in srgb, var(--message-user-action-color) 15%, var(--fond-controle)); }
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
  .cadres-palette {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(6.5rem, 1fr));
    gap: 0.3rem;
    margin-top: 0.5rem;
    max-height: 26rem;
    overflow-y: auto;
  }
  .cadres-couleur {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    background: var(--btn-surface);
    box-shadow: var(--btn-inset);
    border: 1px solid var(--bordure-ui);
    border-radius: var(--rayon-petit);
    padding: 0.3rem 0.45rem;
  }
  .cadres-chip {
    width: 1rem;
    height: 1rem;
    border-radius: 50%;
    border: 1px solid var(--bordure-ui);
    flex-shrink: 0;
  }
  .cadres-nom {
    color: var(--texte);
    font-size: 0.8rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1;
  }
  .cadres-couleur.en-test {
    border-color: var(--accent-ui);
    box-shadow: 0 0 0 1px var(--accent-ui);
  }
  .cadres-test {
    background: var(--btn-surface);
    box-shadow: var(--btn-inset);
    color: var(--texte);
    border: 1px solid var(--bordure-ui);
    border-radius: var(--rayon-petit);
    width: 1.5rem;
    height: 1.5rem;
    padding: 0;
    font-size: 0.6rem;
    line-height: 1;
    cursor: pointer;
    flex-shrink: 0;
    transition: background 0.15s ease, box-shadow 0.15s ease, border-color 0.1s;
  }
  .cadres-test:hover:not(:disabled) {
    background: var(--btn-surface-hover);
    box-shadow: var(--btn-inset-hover);
    border-color: var(--accent-ui);
  }
  .cadres-test.on {
    border-color: var(--accent-ui);
    color: var(--accent-ui);
  }
  .cadres-test:disabled {
    opacity: 0.4;
    cursor: default;
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
    border-radius: var(--rayon);
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
  /* Cases à cocher du pad (Bounce/Zoom/Plein écran/Transparence) :
     horizontales — case à GAUCHE, label à droite (même pattern que
     .ea-check de EcranAttenteModal, pas un .alerte-field vertical). */
  .pad-checks {
    display: flex;
    flex-wrap: wrap;
    gap: 0.3rem 1.1rem;
    margin-bottom: 0.4rem;
  }
  .pad-check {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    font-size: 0.85rem;
    cursor: pointer;
    margin: 0;
    min-width: 0;
  }
  .pad-check input {
    accent-color: var(--accent-ui);
    cursor: pointer;
    margin: 0;
    flex: none;
    width: auto;
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

  /* Tabs — mêmes patterns que CadresModal (bordures var(--accent-ui), pas de
     radius). flex-wrap + nowrap sur les labels : retour à la ligne ENTRE
     les onglets (2e rangée) au lieu de texte tronqué dans les boutons.
     .tab-outil : le bouton « Position des overlays » se détache à droite —
     c'est un outil transversal, pas une destination. */
  .tabs {
    display: flex;
    flex-wrap: wrap;
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
    white-space: nowrap;
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
  .tab-outil {
    margin-left: auto;
  }
  /* Séparateur discret dans le rail gauche : entre la nav outil et le
     contenu contextuel (onglet Commandes). */
  .md-sep {
    border-top: 1px solid var(--bordure-ui);
    margin: 0.15rem 0;
  }

  /* Corps : scroll, colonne de contenu contenue et centrée (pas étirée sur
     les 75rem de la modale) */
  .tab-body {
    flex: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 0.8rem;
    padding: 1rem;
    max-width: 42.5rem;
    width: 100%;
    margin: 0 auto;
  }
  /* Le select police utilise la recette globale (app.css — select). */
  .police-select {
    width: 100%;
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
    height: 0.875rem;
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
  /* Squelette DÉDIÉ du chrono 2 — teinte ambre pour le distinguer du
     squelette partagé (accueil/alertes/commandes/pad). */
  .overlay-skeleton.squelette-chrono {
    background: color-mix(in srgb, var(--message-user-action-color) 12%, transparent);
    border-color: var(--message-user-action-color);
  }
  .overlay-skeleton.squelette-chrono:hover {
    background: color-mix(in srgb, var(--message-user-action-color) 18%, transparent);
  }
  /* Squelette DÉDIÉ du pop de validation des splits — teinte verte
     (couleur des gains speedrun) pour le distinguer du partagé et du
     chrono 2 (ambre). */
  .overlay-skeleton.squelette-splitpop {
    background: color-mix(in srgb, var(--message-ok-color) 12%, transparent);
    border-color: var(--message-ok-color);
  }
  .overlay-skeleton.squelette-splitpop:hover {
    background: color-mix(in srgb, var(--message-ok-color) 18%, transparent);
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
    width: 1.5rem;
    height: 1.5rem;
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
    max-height: 15rem;
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
    max-height: 15rem;
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
    width: 5rem;
    height: 2.8125rem;
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

  /* ===== Onglet Présentation streamer ===== */
  .pres-erreur {
    color: var(--danger-ui, #e5484d);
    font-weight: 600;
    margin: 0 0 0.35rem;
  }
  .pres-search-row {
    display: flex;
    gap: 0.35rem;
    align-items: stretch;
  }
  .pres-search-row .search-input {
    flex: 1;
    min-width: 0;
  }
  .pres-mode-2 {
    grid-template-columns: repeat(2, 1fr);
  }
  .pres-son-row {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    padding: 0.2rem 0;
  }
  .pres-son-label {
    width: 4.2rem;
    font-size: 0.8rem;
    font-weight: 600;
    flex-shrink: 0;
  }
  .pres-son-nom {
    flex: 1;
    min-width: 0;
    font-size: 0.75rem;
    opacity: 0.7;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .pres-live-dot {
    display: inline-block;
    width: 0.5rem;
    height: 0.5rem;
    border-radius: 50%;
    background: #e91916;
    margin-left: 0.3rem;
    vertical-align: middle;
  }
  .pres-at {
    font-size: 0.75rem;
    font-weight: 400;
    opacity: 0.6;
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
    width: 0.5rem;
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
