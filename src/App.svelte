<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { listen, emitTo } from "@tauri-apps/api/event";
  import { loadScene, selectedIdStore, loadedStore, createWidget, createChatWidget, createCameraWidget, createInputViewerWidget, createSpeedrunWidget, sceneStore, syncCapturesOBS } from "./lib/stores/scene";
  import { loadScenesIndex, loadCurrentScene, exportScene, importScene } from "./lib/stores/scenes";
  import { confirmDeleteWidget, cadreModalOpen, interactionModalOpen, moderationModalOpen, titreFondModalOpen, speedrunConfigModalOpen, presetsModalOpen, consoModalOpen, openSectionExplicit, dupliquerSceneModalOpen, ecranAttenteModalOpen, guideLivreOpen, modulesModalOpen } from "./lib/stores/ui";
  import { obsConnect, obsStatus, obsError, obsHost, obsPort, obsPassword, obsStreaming, obsRecording, toggleStreamObs, toggleRecordObs } from "./lib/stores/obs";
  import { initChat, twitchDevice, twitchLogin, youtubeDevice, youtubeLogin, kickSlug, tiktokUsername, connexions, connecterTwitch, deconnecterTwitch } from "./lib/stores/chat";
  import { initTwitchLive } from "./lib/stores/twitchLive";
  import { initPlanning } from "./lib/stores/planning";
  import { connecterKick, deconnecterKick, lireSlugSauve } from "./lib/stores/kick";
  import { connecterYoutube, deconnecterYoutube } from "./lib/stores/youtube";
  import { connecterTiktok, deconnecterTiktok, lireUsernameSauve } from "./lib/stores/tiktok";
  import { initRaccourcis, masquerRaccourcis } from "./lib/stores/raccourcis";
  import { initFenetre } from "./lib/stores/fenetre";
  import { chargerCommunaute, resetCommunaute, chargerCommunauteYoutube } from "./lib/stores/communaute";
  import { initSpeedrun, chargerPreferencesSpeedrun } from "./lib/stores/speedrun";
  import { initObjectifs } from "./lib/stores/objectifs";
  import { tauri } from "./lib/tauri";
  import { injecterFontsCSS } from "./lib/fonts";
  import { get } from "svelte/store";
  import Toolbar from "./lib/components/Toolbar.svelte";
  import CarteEdition from "./lib/components/CarteEdition.svelte";
  import Canvas from "./lib/components/Canvas.svelte";
  import SceneBar from "./lib/components/SceneBar.svelte";
  import DropBar from "./lib/components/DropBar.svelte";
  import TwitchDeviceModal from "./lib/components/TwitchDeviceModal.svelte";
  import ConfirmDeleteWidgetModal from "./lib/components/ConfirmDeleteWidgetModal.svelte";
  import CadresModal from "./lib/components/CadresModal.svelte";
  import InteractionViewerModal from "./lib/components/InteractionViewerModal.svelte";
  import ModerationModal from "./lib/components/ModerationModal.svelte";
  import TitreModal from "./lib/components/TitreModal.svelte";
  import TitreFondModal from "./lib/components/TitreFondModal.svelte";
  import SpeedrunConfigModal from "./lib/components/SpeedrunConfigModal.svelte";
  import PresetsModal from "./lib/components/PresetsModal.svelte";
  import ConsoModal from "./lib/components/ConsoModal.svelte";
  import DupliquerSceneModal from "./lib/components/DupliquerSceneModal.svelte";
  import EcranAttenteModal from "./lib/components/EcranAttenteModal.svelte";
  import ModulesModal from "./lib/components/ModulesModal.svelte";
  import GuideLivre from "./lib/components/GuideLivre.svelte";
  import Drapeau from "./lib/components/Drapeau.svelte";
  import { chargerPresets } from "./lib/stores/presets";
  import { initWelcome, chargerWelcome } from "./lib/stores/welcome";
  import { initBandeau, chargerBandeau } from "./lib/stores/bandeau";
  import { chargerAlertes } from "./lib/stores/alertes";
  import { initPositionOverlay, chargerPositionOverlay } from "./lib/stores/positionOverlay";
  import { initPad, chargerPad } from "./lib/stores/padNumerique";
  import { initPresentation, chargerPresentation } from "./lib/stores/presentation";
  import { ecranAttenteStore, initEcranAttente, chargerEcranAttente, toggleEcranAttente } from "./lib/stores/ecranAttente";
  import { jcActif, initJustChatting, chargerJustChatting, toggleJustChatting } from "./lib/stores/justChatting";
  import { fondSidebar } from "./lib/stores/fondSidebar";
  import { fondDash } from "./lib/stores/fondDash";
  import { gradientCadre } from "./lib/cadres/registre";
  import { langue, LANGUES, setLangue, initLangue, t, type Langue } from "./lib/stores/langue";
  import logoSOS from "./assets/logo/LogoSOSDashboardSmallSize.png";

  let serverError = $state<string | null>(null);
  let serverOk = $state(false);
  // Dropdown langue topbar (bouton « Langue » du groupe outils à droite).
  let languesOuvert = $state(false);
  // Timer auto-reconnect OBS (nettoyé au destroy/HMR pour éviter l'accumulation
  // d'intervals lors des reloads Vite en dev).
  let obsReconnectTimer: ReturnType<typeof setInterval> | null = null;

  onDestroy(() => {
    if (obsReconnectTimer) clearInterval(obsReconnectTimer);
  });

  // États dérivés pour le bandeau unique (Twitch / OBS / :4321).
  let cx = $derived($connexions);
  let login = $derived($twitchLogin);
  let ytLogin = $derived($youtubeLogin);
  let kickSl = $derived($kickSlug);
  let ttUser = $derived($tiktokUsername);
  let obsSt = $derived($obsStatus);
  let obsErr = $derived($obsError);
  let obsStream = $derived($obsStreaming);
  let obsRecord = $derived($obsRecording);
  // État écran d'attente (BRB) : pillule header quand actif + pastille barre
  // de raccourcis. temps_restant descend à chaque tick Rust 1 Hz — l'effect
  // de push raccourcis suit eaActif (bool) pour ne pas émettre à chaque tick.
  let ea = $derived($ecranAttenteStore);
  let eaActif = $derived(ea?.actif ?? false);
  // Just Chatting : pillule header quand actif + pastille barre raccourcis.
  let jcOn = $derived($jcActif);

  /// Formate des secondes → "M:SS" ou "H:MM:SS" (pillule attente header).
  function formatTempsAttente(total: number): string {
    const t = Math.max(0, Math.floor(total));
    const h = Math.floor(t / 3600);
    const m = Math.floor((t % 3600) / 60);
    const s = t % 60;
    if (h > 0) {
      return `${h}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
    }
    return `${m}:${String(s).padStart(2, "0")}`;
  }

  // Couleur EFFECTIVE du cadre SVG unifié → tout le chrome la suit : poussée
  // dans --cadre-couleur / --cadre-couleur-fin sur :root, consommée par
  // --accent-ui (bordures/hovers/focus), --fond-ambiance, .fond-decor et
  // le bouton « Vos scènes ». Résolue via gradientCadre() — le même
  // resolver que le rendu : couleur/couleurFin du cadre unifié.
  // Source unique : cadreWidget (champ canonique).
  // Image de fond du dashboard (Personnalisation → « Image de fond ») —
  // servie par :4321 comme les médias de scène, sous tout le décor.
  let fondSidebarUrl = $derived($fondSidebar ? "http://127.0.0.1:4321/" + $fondSidebar : null);
  // Fond image du CHROME dashboard (bouton « Fond du dashboard » en bas du
  // panneau latéral) — posé en background de <main> : il transparaît dans
  // les zones transparentes du chrome (header, sidebar, DropBar, marges
  // autour du canvas) et le .canvas opaque le recouvre → la composition
  // n'est jamais touchée. Servi par :4321 comme les médias de scène.
  let fondDashUrl = $derived($fondDash ? "http://127.0.0.1:4321/" + $fondDash : null);

  let cadreActif = $derived(
    $sceneStore.cadreWidget?.actif ? $sceneStore.cadreWidget : null
  );
  let cadreCouleur = $derived(cadreActif ? gradientCadre(cadreActif).from : null);
  let cadreCouleurFin = $derived(cadreActif ? gradientCadre(cadreActif).to : null);
  $effect(() => {
    const root = document.documentElement.style;
    if (cadreCouleur) root.setProperty("--cadre-couleur", cadreCouleur);
    else root.removeProperty("--cadre-couleur");
    if (cadreCouleurFin) root.setProperty("--cadre-couleur-fin", cadreCouleurFin);
    else root.removeProperty("--cadre-couleur-fin");
    return () => {
      root.removeProperty("--cadre-couleur");
      root.removeProperty("--cadre-couleur-fin");
    };
  });

  // ===== Communauté : chargement réactif =====
  // $effect réactif : charge la communauté quand Twitch passe connecté,
  // reset quand déconnecté. Plus fiable qu'un listener d'event async
  // (pas de race condition avec l'enregistrement). Vit dans App.svelte
  // (toujours monté) pour précharger les données même si la modale
  // Modération & Rôles n'est pas ouverte (la section Viewers en dépend).
  $effect(() => {
    if (cx.twitch) {
      chargerCommunaute();
    } else {
      resetCommunaute();
    }
  });

  // $effect YouTube : charge la communauté YouTube quand connecté.
  $effect(() => {
    if (cx.youtube) {
      chargerCommunauteYoutube();
    }
  });

  // ===== Sélecteur de langue (topbar, groupe outils à droite) =====
  // Pattern « Vos scènes » : bouton toggle + dropdown absolu ; fermeture par
  // sélection, clic extérieur (fermerLangues) ou Escape (onSupprKey).
  function toggleLangues() {
    languesOuvert = !languesOuvert;
  }
  function fermerLangues() {
    languesOuvert = false;
  }
  async function choisirLangue(id: Langue) {
    fermerLangues();
    await setLangue(id);
  }

  // ===== Barre de raccourcis : dispatch des actions =====
  // La fenêtre raccourcis émet "raccourcis:action" {id} — le mapping ci-dessous
  // appelle les fonctions EXISTANTES des stores (mêmes que la Toolbar).
  // Aucune logique métier dans la barre : elle n'est qu'un relais.
  async function dispatchRaccourcis(id: string) {
    const c = get(connexions);
    switch (id) {
      case "widget-media": createWidget(); break;
      case "widget-chat": createChatWidget(); break;
      case "widget-camera":
        // Même garde que le bouton Toolbar (singleton caméra).
        if (!get(sceneStore).widgets.some((w) => w.type === "camera")) {
          createCameraWidget();
        }
        break;
      case "widget-input-viewer": createInputViewerWidget(); break;
      case "widget-speedrun": createSpeedrunWidget(); break;
      case "connexion-twitch":
        // connecterTwitch ouvre la modale Device Flow sur le dashboard.
        if (c.twitch) await deconnecterTwitch(); else await connecterTwitch();
        break;
      case "connexion-youtube":
        if (c.youtube) await deconnecterYoutube(); else await connecterYoutube();
        break;
      case "connexion-kick":
        if (c.kick) {
          await deconnecterKick();
        } else {
          // Pas d'input dans la barre : slug courant puis slug sauvegardé.
          // Aucun des deux → ouvrir la section Connexions pour le saisir.
          const slug = get(kickSlug) ?? (await lireSlugSauve());
          if (slug) await connecterKick(slug);
          else openSectionExplicit("connexions");
        }
        break;
      case "connexion-tiktok":
        if (c.tiktok) {
          await deconnecterTiktok();
        } else {
          const username = get(tiktokUsername) ?? (await lireUsernameSauve());
          if (username) await connecterTiktok(username);
          else openSectionExplicit("connexions");
        }
        break;
      case "connexion-obs":
        // OBS est one-shot (pas de déconnexion) : clic = re-connect + re-sync,
        // comme le bouton « Connecter » de la section OBS.
        try {
          await obsConnect(get(obsHost), get(obsPort), get(obsPassword));
          await syncCapturesOBS();
        } catch {
        }
        break;
      case "obs-stream":
        // Toggle sortie OBS stream (one-shot : connect → ToggleStream → close).
        // Erreur traduite dans obsError par le store — pas de throw ici.
        try { await toggleStreamObs(); } catch {}
        break;
      case "obs-record":
        try { await toggleRecordObs(); } catch {}
        break;
      case "modale-interactions": interactionModalOpen.set(true); break;
      case "modale-moderation": moderationModalOpen.set(true); break;
      case "modale-cadres": cadreModalOpen.set(true); break;
      case "ecran-attente":
        // Toggle lancer/arrêter l'écran d'attente (chrono + overlay diffusion
        // + routage audio OBS). Erreur remontée dans ecranAttenteErreur.
        try { await toggleEcranAttente(); } catch {}
        break;
      case "just-chatting":
        // Toggle overlay JC (caméra + chat seuls en diffusion, non
        // destructif). Non-fatal OBS offline — même fonction que la Toolbar.
        try { await toggleJustChatting(); } catch {}
        break;
      case "systeme-masquer": await masquerRaccourcis(); break;
    }
  }

  // Pousse l'état des connexions vers la barre (pastilles vertes — affichage
  // seul). Appelé par le $effect réactif ET par "raccourcis:ready" (la barre
  // annonce que ses listeners sont prêts — évite l'état périmé quand elle est
  // créée après le dernier changement d'état).
  function pushRaccourcisConnexions() {
    const c = get(connexions);
    void emitTo("raccourcis", "raccourcis:connexions", {
      twitch: c.twitch,
      youtube: c.youtube,
      kick: c.kick,
      tiktok: c.tiktok,
      obs: get(obsStatus) === "connected",
      // Sorties OBS : pastilles stream/record de la barre (data-plat homonymes).
      stream: get(obsStreaming),
      record: get(obsRecording),
      // Écran d'attente : pastille du bouton BRB (vert = chrono en cours).
      attente: get(ecranAttenteStore)?.actif ?? false,
      // Just Chatting : pastille du bouton (vert = overlay actif).
      justchatting: get(jcActif),
    }).catch(() => {});
  }

  $effect(() => {
    // Référence réactive : relire les stores à chaque changement.
    void cx;
    void obsSt;
    void obsStream;
    void obsRecord;
    void eaActif;
    void jcOn;
    pushRaccourcisConnexions();
  });

  // ===== Boutons topbar : sorties OBS (stream / record) =====
  // busy par bouton = anti double-clic (chaque toggle = UNE connexion OBS
  // one-shot). L'erreur est traduite dans obsError par le store → catch
  // silencieux ici (même pattern que dispatchRaccourcis obs-stream/record).
  let streamBusy = $state(false);
  let recordBusy = $state(false);
  async function onToggleStreamTop() {
    if (streamBusy) return;
    streamBusy = true;
    try {
      await toggleStreamObs();
    } catch {
    } finally {
      streamBusy = false;
    }
  }
  async function onToggleRecordTop() {
    if (recordBusy) return;
    recordBusy = true;
    try {
      await toggleRecordObs();
    } catch {
    } finally {
      recordBusy = false;
    }
  }

  // ===== Boutons topbar : Arrêter / Redémarrer / Refresh OBS =====
  async function onArreter() {
    try {
      await tauri.appArreter();
    } catch (e) {
      console.error("onArreter:", e);
    }
  }
  async function onRedemarrer() {
    try {
      await tauri.appRedemarrer();
    } catch (e) {
      console.error("onRedemarrer:", e);
    }
  }
  async function onRefreshObs() {
    await refreshDiffusion();
  }

  // Refresh SOS-Diffusion : appelle obs_refresh_diffusion (refreshnocache).
  // Appelé après obsConnect OK quand serveur :4321 est prêt.
  async function refreshDiffusion() {
    try {
      await tauri.obsRefreshDiffusion(
        get(obsHost), parseInt(get(obsPort), 10), get(obsPassword)
      );
    } catch {
    }
  }

  // Auto-connect OBS + refresh SOS-Diffusion + sync captures.
  // Appelé quand serveur :4321 est prêt (event ou fetch fallback).
  // Le sync captures APRÈS obsConnect garantit : SOS-Trou-* actifs + source
  // "SOS-Caméra" du widget caméra (auto-guérison si créée sans OBS). Corrige
  // aussi la race boot : le sync du onMount (step 6) peut s'exécuter avant la
  // fin de obsConnect → ici on est sûr que OBS est connecté.
  async function bootObs() {
    try {
      await obsConnect(get(obsHost), get(obsPort), get(obsPassword));
      // obsConnect a mis obsStatus=connected. Refresh direct (pas de race).
      await refreshDiffusion();
      // Sync captures + masque caméra : non-fatal (OBS offline ou scène SOS
      // absente → skip).
      await syncCapturesOBS();
    } catch {
    }
  }

  // Fallback : fetch no-cors pour détecter si :4321 est déjà up.
  // Si l'event server_ready a été émis avant le listener, on le rate.
  // Ce fetch comble le trou : si :4321 répond, on boot OBS tout de suite.
  async function checkServerUp(): Promise<boolean> {
    try {
      await fetch("http://127.0.0.1:4321/", { mode: "no-cors" });
      return true;
    } catch {
      return false;
    }
  }

  onMount(async () => {
    // 0. Injecter les @font-face des polices Google Font pour les titres de
    //    widgets (servies par :4321 sur /fonts/{name}).
    injecterFontsCSS();

    // 0b. Langue UI : lit langue.json (choix persisté) ou navigator.language
    //     (fallback "fr"). Avant tout le reste — les libellés du boot
    //     (onboarding presets, erreurs) s'affichent dans la bonne langue.
    await initLangue();

    // 1. Listeners server_ready + server_error EN PREMIER (avant initChat).
    //    Si le serveur bind avant le listener, l'event est perdu → fallback fetch.
    await listen<string>("server_error", (e) => {
      serverError = e.payload;
      serverOk = false;
    });

    await listen<string>("server_ready", () => {
      serverOk = true;
      serverError = null;
      bootObs();
    });

    // 2. Le boot OBS est géré par bootObs() ci-dessus (server_ready / fallback fetch).

    // 3. initChat (listeners twitch:connecte, chat:message, etc.)
    await initChat();

    // 3a. initTwitchLive (listeners twitch:automod + twitch:need_reauth —
    //     file AutoMod alimentée même modale fermée). Garde "une fois" dans le store.
    await initTwitchLive();

    // 3a2. initPlanning (listener twitch:deconnecte → reset des segments
    //      planning affichés). Garde "une fois" dans le store.
    await initPlanning();

    // 3b. initWelcome (listeners welcome:etat + welcome:attribution-progress)
    //     + charge l'état initial de la queue + registre.
    await initWelcome();
    await chargerWelcome();

    // 3c. initBandeau (listener bandeau:etat) + charge l'état initial.
    await initBandeau();
    await chargerBandeau();

    // 3d. Alertes : charge la config (les événements — follows via EventSub
    //     WebSocket temps réel, subs/bits/raids via IRC — sont gérés côté Rust).
    await chargerAlertes();

    // 3e. Speedrun Splitter : écoute l'event speedrun:event + charge les
    //     préférences sauvegardées (chemins ASL/LSS + settings).
    await initSpeedrun();
    await chargerPreferencesSpeedrun();

    // 3f. Objectifs : listeners communaute:update + viewers:update (valeurs
    //     live pour la carte d'édition et l'aperçu dashboard des widgets goal).
    await initObjectifs();

    // 3e. Squelette de position unifié (clip de bienvenue + alertes) :
    //     init listener + charge la config initiale depuis Rust.
    await initPositionOverlay();
    await chargerPositionOverlay();

    // 3f. Pad numérique (16 touches Numpad × 3 plages → overlay diffusion) :
    //     init listeners (pad:etat, pad:touche, pad:touche-stop, pad:plage-changee)
    //     + charge la config initiale. L'audio local est joué côté dashboard.
    await initPad();
    await chargerPad();

    // 3f-bis. Écran d'attente (BRB) : init listener (ecran-attente:etat,
    //     émis à chaque tick Rust 1 Hz / transition) + charge l'état initial.
    //     Le média est rendu côté diffusion uniquement (consigne utilisateur).
    await initEcranAttente();
    await chargerEcranAttente();

    // 3f-quater. Présentation streamer : listener presentation:etat (CRUD
    //     favoris + show/hide + bascule de mode) + état initial (favoris
    //     persistés + présentation en cours — null en pratique, RAM only).
    await initPresentation();
    await chargerPresentation();

    // 3f-ter. Just Chatting : listener (just-chatting:etat) + hook post-sync
    //     captures (re-envoie la scène dérivée si la scène change pendant
    //     que le mode est actif) + état initial (OFF au boot, runtime).
    await initJustChatting();
    await chargerJustChatting();

    // 3g. Barre de raccourcis : état initial (visible/bord/moniteur) + listener
    //     des actions émises par la fenêtre + handshake "ready" (la barre peut
    //     être créée avant que le dashboard ait fini son init → elle demande
    //     l'état des connexions pour ses pastilles).
    await initRaccourcis();
    // Fenêtre principale : préférence persistée (écran + mode de démarrage)
    // pour les contrôles de la section Ergonomie.
    await initFenetre();
    await listen<{ id: string }>("raccourcis:action", (e) => {
      void dispatchRaccourcis(e.payload.id);
    });
    await listen("raccourcis:ready", () => pushRaccourcisConnexions());

    // 4. Fallback : si :4321 déjà up (event manqué), boot OBS maintenant.
    const up = await checkServerUp();
    if (up && !serverOk) {
      serverOk = true;
      serverError = null;
      bootObs();
    }

    // 4b. Auto-reconnect OBS : tant que non connecté, retente toutes les 20s.
    //     L'utilisateur peut lancer OBS APRÈS l'app → le voyant top bar passe
    //     vert tout seul (point vert + "OBS"), l'en-tête sidebar quitte le
    //     rouge. Guard : skip si connected/connecting (pas de double tentative
    //     avec un clic manuel "Connecter"). Chaque succès enchaîne refresh +
    //     sync captures (bootObs est idempotent — OBS connect sans doublon).
    obsReconnectTimer = setInterval(() => {
      const s = get(obsStatus);
      if (s !== "connected" && s !== "connecting") {
        void bootObs();
      }
    }, 20_000);

    // 5. Charge la scène initiale + index scènes + scène courante
    await loadScene();
    await loadScenesIndex();
    await loadCurrentScene();

    // 5b. Onboarding presets : au PREMIER lancement (onboarding.json absent),
    //     ouvre la galerie de scènes modèles en mode "onboarding" — le user
    //     lambda démarre avec une scène prête au lieu d'un canvas vide.
    //     Non-bloquant : erreur de lecture = onboarding considéré fait (Rust).
    try {
      if (!(await tauri.onboardingEtat())) {
        await chargerPresets();
        presetsModalOpen.set("onboarding");
      }
    } catch {
      // Erreur invoke → ne pas bloquer le boot
    }

    // 6. Sync captures OBS : géré par bootObs() après obsConnect réussi (via
    //    server_ready / fallback fetch / auto-reconnect 20s). L'ancien appel
    //    one-shot ici était redondant et échouait bruyamment quand OBS offline.
  });

  // Suppr : demande confirmation suppression widget (même flux que bouton Toolbar).
  // Attaché via <svelte:window onkeydown> (cf. template) — pas window.addEventListener —
  // pour (1) s'attacher synchrone au mount (pas après les awaits du onMount),
  // (2) tourner dans le contexte d'événement Svelte (la mise à jour du store
  //     confirmDeleteWidget déclenche bien le re-render, comme le bouton Toolbar),
  // (3) être nettoyé automatiquement au destroy/HMR.
  // Aucun conflit avec le onkeydown de SceneBar (qui ne gère que Escape et ne
  // propage ni ne prévient rien) : plusieurs <svelte:window onkeydown> coexistent.
  // Ignore si focus dans un input/contenteditable (rename scène, etc.).
  function onSupprKey(e: KeyboardEvent) {
    // Escape : ferme le dropdown langue (pattern « Vos scènes » — aucune
    // propagation à prévenir, plusieurs <svelte:window onkeydown> coexistent).
    if (e.key === "Escape") {
      if (languesOuvert) fermerLangues();
      return;
    }
    if (e.key !== "Delete") return;
    const t = e.target as HTMLElement;
    if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.isContentEditable)) return;
    if (!get(loadedStore)) return;
    const id = get(selectedIdStore);
    if (!id) return;
    e.preventDefault();
    // Ouvre la modale de confirmation (ConfirmDeleteWidgetModal montée ci-dessous).
    confirmDeleteWidget.set(true);
  }
</script>

<!-- Clic hors du menu Langue → ferme le dropdown.
     onkeydown : touche Suppr → confirmation suppression widget (handler onSupprKey). -->
<svelte:window onclick={fermerLangues} onkeydown={onSupprKey} />

<main
  class:fond-dash={!!fondDashUrl}
  style:--fond-dash-img={fondDashUrl ? `url('${fondDashUrl}')` : null}
>
  <!-- Décor aurora : arcs orbitaux derrière tout le chrome (cf. app.css). -->
  <div class="fond-decor" aria-hidden="true">
    {#if fondSidebarUrl}
      <div class="fond-image" style="background-image: url('{fondSidebarUrl}')"></div>
    {/if}
    <div class="orb orb-droite"></div>
    <div class="orb orb-gauche-a"></div>
    <div class="orb orb-gauche-b"></div>
    <div class="ligne"></div>
    <div class="boule b1"></div>
    <div class="boule b2"></div>
    <div class="boule b3"></div>
    <div class="boule b4"></div>
    <div class="boule b5"></div>
    <div class="boule b6"></div>
    <div class="boule b7"></div>
    <div class="boule b8"></div>
  </div>
  <header>
    <img class="logo-img" src={logoSOS} alt="StreamOS" />
    <!-- Groupe 1 : identité + connexions — chaque état est un bouton qui
         ouvre sa section de la barre latérale (connexions / obs). -->
    <div class="tb-groupe tb-connexions">
      <button
        class="indicateur plat-twitch tb-ico"
        class:on={cx.twitch}
        class:off={!cx.twitch}
        onclick={() => openSectionExplicit("connexions")}
        title={cx.twitch && login ? `Twitch : ${login}` : "Twitch"}
        aria-label={cx.twitch && login ? `Twitch : ${login}` : "Twitch"}
      >
        <svg class="ico" viewBox="0 0 16 16" aria-hidden="true"><path fill="#9146ff" d="M3 1L1.5 3.5V13H4.5V15H6L8 13H10.5L13.5 10V1H3ZM12 9.5L10 11.5H7L5 13.5V11.5H3V2H12V9.5Z"/><rect x="9" y="4" width="1" height="3" fill="#9146ff"/><rect x="6" y="4" width="1" height="3" fill="#9146ff"/></svg>
      </button>
      <button
        class="indicateur plat-kick tb-ico"
        class:on={cx.kick}
        class:off={!cx.kick}
        onclick={() => openSectionExplicit("connexions")}
        title={cx.kick && kickSl ? `Kick : ${kickSl}` : "Kick"}
        aria-label={cx.kick && kickSl ? `Kick : ${kickSl}` : "Kick"}
      >
        <svg class="ico" viewBox="0 0 24 24" aria-hidden="true"><path fill="#53fc18" d="M5 2h4v6h2V6h2V4h2V2h4v6h-2v2h-2v4h2v2h2v6h-4v-2h-2v-2h-2v4H5z"/></svg>
      </button>
      <button
        class="indicateur plat-youtube tb-ico"
        class:on={cx.youtube}
        class:off={!cx.youtube}
        onclick={() => openSectionExplicit("connexions")}
        title={cx.youtube && ytLogin ? `YouTube : ${ytLogin}` : "YouTube"}
        aria-label={cx.youtube && ytLogin ? `YouTube : ${ytLogin}` : "YouTube"}
      >
        <svg class="ico" viewBox="0 0 24 24" aria-hidden="true"><rect x="2" y="5" width="20" height="14" rx="4" fill="#ff0033"/><path d="M10 9.5l5 2.5-5 2.5z" fill="#fff"/></svg>
      </button>
      <button
        class="indicateur plat-tiktok tb-ico"
        class:on={cx.tiktok}
        class:off={!cx.tiktok}
        onclick={() => openSectionExplicit("connexions")}
        title={cx.tiktok && ttUser ? `TikTok : ${ttUser}` : "TikTok"}
        aria-label={cx.tiktok && ttUser ? `TikTok : ${ttUser}` : "TikTok"}
      >
        <svg class="ico" viewBox="0 0 24 24" aria-hidden="true"><path fill="#25f4ee" d="M16.5 3c.3 1.8 1.6 3.2 3.5 3.5v3.1c-1.4 0-2.7-.5-3.7-1.2v6.3a5.6 5.6 0 1 1-5.6-5.6c.4 0 .8 0 1.2.1v3.2a2.5 2.5 0 1 0 1.4 2.3V3h3.2z"/></svg>
      </button>
      <button
        class="indicateur plat-obs tb-ico"
        class:on={obsSt === "connected"}
        class:off={obsSt !== "connected"}
        class:err={obsSt === "idle" || obsSt === "error"}
        onclick={() => openSectionExplicit("obs")}
        title={obsErr ?? $t("app.obsTitle")}
        aria-label={obsErr ?? $t("app.obsTitle")}
      >
        <svg class="ico" viewBox="0 0 24 24" fill="none" stroke="#e8e8e8" stroke-width="1.8" stroke-linecap="round" aria-hidden="true"><circle cx="12" cy="12" r="9"/><path d="M12 7a5 5 0 0 1 4.3 2.5M16.8 14a5 5 0 0 1-4.8 3M7.2 14a5 5 0 0 1-.2-4.3"/></svg>
      </button>
    </div>
    <!-- Groupe 2 : diffusion + transport. Diffusion = statut pur (bouton
         désactivé — pas de navigation). -->
    <div class="tb-groupe tb-live">
      <button
        class="indicateur plat-diffusion tb-ico"
        class:on={serverOk}
        class:off={!serverOk}
        class:err={!!serverError}
        disabled
        title={serverError ?? $t("app.diffusionTitle")}
        aria-label={serverError ?? $t("app.diffusionTitle")}
      >
        <svg class="ico" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="5" cy="19" r="1.6" fill="currentColor" stroke="none"/><path d="M3 11a10 10 0 0 1 10 10"/><path d="M3 4a17 17 0 0 1 17 17"/></svg>
      </button>
      <!-- Pillule écran d'attente : visible seulement quand le chrono tourne
           (état poussé par Rust à 1 Hz). Clic → ouvre la modale de contrôle
           (pause / arrêt / config). -->
      {#if ea?.actif}
        <button
          class="indicateur plat-attente"
          class:pause={ea.en_pause}
          title={ea.en_pause
            ? $t("app.eaTitlePause")
            : $t("app.eaTitleActif")}
          onclick={() => ecranAttenteModalOpen.set(true)}
        >
          <svg class="ico" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 3"/></svg>
          {ea.en_pause ? $t("app.attentePause") : $t("app.attente")} {formatTempsAttente(ea.temps_restant)}
        </button>
      {/if}
      <!-- Pillule Just Chatting : visible seulement quand l'overlay est
           actif. Clic → coupe le mode (même toggle que la Toolbar). -->
      {#if jcOn}
        <button
          class="indicateur"
          title={$t("toolbar.justChattingTitle")}
          onclick={() => { void toggleJustChatting(); }}
        >
          <svg class="ico" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="2" y="5" width="12" height="14" rx="2"/><circle cx="8" cy="11" r="2.5"/><path d="M15 10h6.5v6H19l-2 2v-2h-2"/></svg>
          {$t("toolbar.justChatting")}
        </button>
      {/if}
      <!-- Sorties OBS : stream + record (vert = prêt à démarrer, rouge = actif) -->
      <button
        class="output-btn"
        class:actif={obsStream}
        onclick={onToggleStreamTop}
        disabled={obsSt !== "connected" || streamBusy}
        title={obsSt !== "connected"
          ? $t("app.obsDabord")
          : obsStream
            ? $t("app.streamStop")
            : $t("app.streamStart")}
        aria-label={obsStream ? $t("app.streamStop") : $t("app.streamStart")}
      >
        <svg class="ico" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4.93 19.07a10 10 0 0 1 0-14.14"/><path d="M19.07 4.93a10 10 0 0 1 0 14.14"/><path d="M7.76 16.24a6 6 0 0 1 0-8.49"/><path d="M16.24 7.76a6 6 0 0 1 0 8.49"/><circle cx="12" cy="12" r="1.5" fill="currentColor" stroke="none"/></svg>
      </button>
      <button
        class="output-btn"
        class:actif={obsRecord}
        onclick={onToggleRecordTop}
        disabled={obsSt !== "connected" || recordBusy}
        title={obsSt !== "connected"
          ? $t("app.obsDabord")
          : obsRecord
            ? $t("commun.arreterEnregistrement")
            : $t("commun.demarrerEnregistrement")}
        aria-label={obsRecord ? $t("commun.arreterEnregistrement") : $t("commun.demarrerEnregistrement")}
      >
        <svg class="ico" viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="7" fill="none" stroke="currentColor" stroke-width="1.8"/><circle cx="12" cy="12" r="3.5" fill="#f43f5e"/></svg>
      </button>
    </div>
    <!-- Groupe outils : scènes (Vos scènes = dropdown SceneBar, + Scène,
         Exporter, Importer) puis contrôle app (Arrêter / Redémarrer /
         Refresh OBS) puis Langue. Poussé à droite (margin-left: auto). -->
    <div class="tb-groupe tb-outils">
      <SceneBar />
      <button class="indicateur tb-action" onclick={() => presetsModalOpen.set("creation")} title={$t("sceneBar.nouvelle")} aria-label={$t("sceneBar.nouvelle")}>+ <span class="tb-label">{$t("sceneBar.nouvelle")}</span></button>
      <button class="indicateur tb-action" onclick={exportScene} title={$t("commun.exporter")} aria-label={$t("commun.exporter")}>
        <svg class="ico" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="17 8 12 3 7 8"/><line x1="12" y1="3" x2="12" y2="15"/></svg>
        <span class="tb-label">{$t("commun.exporter")}</span>
      </button>
      <button class="indicateur tb-action" onclick={importScene} title={$t("commun.importer")} aria-label={$t("commun.importer")}>
        <svg class="ico" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>
        <span class="tb-label">{$t("commun.importer")}</span>
      </button>
      <button class="ctrl-btn ctrl-arreter" onclick={onArreter} title={$t("app.arreterApp")} aria-label={$t("app.arreterApp")}>
        <svg class="ico" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><rect x="7" y="7" width="10" height="10" rx="1.5"/></svg>
      </button>
      <button class="ctrl-btn ctrl-redemarrer" onclick={onRedemarrer} title={$t("app.redemarrerApp")} aria-label={$t("app.redemarrerApp")}>
        <svg class="ico" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M21 12a9 9 0 1 1-2.64-6.36"/><path d="M21 3v5h-5"/></svg>
      </button>
      <button class="ctrl-btn ctrl-refresh" onclick={onRefreshObs} title={$t("app.refreshObs")} aria-label={$t("app.refreshObs")}>
        <svg class="ico" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8"/><path d="M21 3v5h-5"/><path d="M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16"/><path d="M3 21v-5h5"/></svg>
      </button>
      <!-- Sélecteur de langue : petit bouton topbar du groupe outils.
           Dropdown au clic — pattern « Vos scènes » (fermeture : sélection,
           clic extérieur via fermerLangues, Escape via onSupprKey). Les noms
           sont NATIFS (jamais traduits) pour que chacun retrouve sa langue
           écrite chez lui. -->
      <div
        class="langue-wrap"
        role="presentation"
        onclick={(e) => e.stopPropagation()}
        onkeydown={(e) => e.stopPropagation()}
      >
        <button class="langue-btn" onclick={toggleLangues} title={$t("langue.bouton")} aria-label={$t("langue.bouton")}><span class="lg-full">{$t("langue.bouton")}</span><span class="lg-court" aria-hidden="true">{$t("langue.bouton").slice(0, 1)}</span></button>
        {#if languesOuvert}
          <div class="langues-dropdown ctx-menu">
            {#each LANGUES as l (l.id)}
              <button
                class="langue-item ctx-item"
                class:active={$langue === l.id}
                onclick={() => choisirLangue(l.id)}
              >
                <Drapeau id={l.id} />
                <span class="langue-nom">{l.nom}</span>
                {#if $langue === l.id}<span class="langue-coche">✓</span>{/if}
              </button>
            {/each}
          </div>
        {/if}
      </div>
    </div>
  </header>

  <div class="row">
    <div class="sidebar-col">
      <CarteEdition />
      <Toolbar />
    </div>
    <div class="canvas-col">
      <DropBar />
      <Canvas />
    </div>
  </div>
</main>

{#if $twitchDevice || $youtubeDevice}
  <TwitchDeviceModal />
{/if}

<!-- Modale confirmation suppression widget (bouton Toolbar + touche Suppr) -->
<ConfirmDeleteWidgetModal />

<!-- Modale galerie de cadres SVG (section Personnalisation) -->
{#if $cadreModalOpen}
  <CadresModal />
{/if}

<!-- Modale Interactions chat (onglets : Clip de bienvenue / Bandeau / Alertes) -->
{#if $interactionModalOpen}
  <InteractionViewerModal onFermer={() => interactionModalOpen.set(false)} />
{/if}

<!-- Modale Modération & Rôles (onglets : Twitch / Kick / YouTube / TikTok / Trovo) -->
{#if $moderationModalOpen}
  <ModerationModal onFermer={() => moderationModalOpen.set(false)} />
{/if}

<!-- Modale d'édition du titre d'un widget (double-clic haut/bas d'un widget) -->
<TitreModal />

<!-- Modale d'édition du titre du fond de l'application (bouton Toolbar) -->
{#if $titreFondModalOpen}
  <TitreFondModal />
{/if}

<!-- Modale de configuration ASL (auto-splitter) — s'ouvre auto au chargement ASL -->
{#if $speedrunConfigModalOpen}
  <SpeedrunConfigModal onFermer={() => speedrunConfigModalOpen.set(false)} />
{/if}

<!-- Modale galerie de presets de scènes (bouton « + Scène » / 1er lancement) -->
{#if $presetsModalOpen}
  <PresetsModal mode={$presetsModalOpen} />
{/if}

<!-- Modale de duplication d'une scène (clic droit pastille → « Dupliquer ») -->
{#if $dupliquerSceneModalOpen}
  <DupliquerSceneModal />
{/if}

<!-- Modale Diagnostic (consommation : CPU/RAM par processus + sous-systèmes +
     activité — polling 1 Hz uniquement tant qu'elle est montée) -->
{#if $consoModalOpen}
  <ConsoModal onFermer={() => consoModalOpen.set(false)} />
{/if}

<!-- Modale Écran d'attente (BRB) — gère son propre {#if $ecranAttenteModalOpen}
     interne (même pattern que TitreModal). Média rendu diffusion uniquement. -->
<EcranAttenteModal />

<!-- Modale Modules HUD — la valeur porte le module sélectionné
     ("horloge" via le lanceur Toolbar, id du module au clic canvas). -->
{#if $modulesModalOpen}
  <ModulesModal />
{/if}

<!-- Overlay plein écran « Guide du streamer » (livre feuilletable) —
     lancé depuis la section Ergonomie du pied de sidebar. -->
{#if $guideLivreOpen}
  <GuideLivre />
{/if}

<style>
  main {
    display: flex;
    flex-direction: column;
    height: 100%;
    /* Garde-fou anti scroll horizontal : le clip vit sur main (pas sur
       header) pour ne pas rogner les dropdowns du header (Langue,
       Vos scènes) qui débordent verticalement de la barre. */
    overflow: clip;
  }
  /* Fond image du chrome (bouton « Fond du dashboard ») : image + voile
     sombre empilés sur TOUT le main — le chrome transparent (header,
     sidebar, DropBar, marges autour du canvas) la laisse transparaître,
     le .canvas opaque la recouvre. Le voile (--fond à 55 %) garde la
     lisibilité du texte, même esprit que .fond-image::after (app.css). */
  main.fond-dash {
    background-image:
      linear-gradient(
        color-mix(in srgb, var(--fond) 55%, transparent),
        color-mix(in srgb, var(--fond) 55%, transparent)
      ),
      var(--fond-dash-img);
    background-size: cover;
    background-position: center;
  }
  header {
    flex-shrink: 0;
    display: flex;
    align-items: center;
    gap: 0.5rem;
    flex-wrap: nowrap;
    padding: 0.35rem 0.75rem;
    border-bottom: 1px solid var(--bordure-ui);
    background: transparent; /* laisse transparaître le fond d'ambiance */
    font-size: 0.85rem;
  }
  /* Logo StreamOS : PNG officiel (fond transparent). L'artwork n'occupe
     que ~le tiers central du PNG (lignes 116–241/369) ; les marges
     négatives recentrent l'artwork sur la barre — le haut rogné par le
     bord du webview est transparent. Boîte de layout ~32px : le header
     garde sa hauteur. pointer-events: none : la zone transparente qui
     déborde sous la barre ne doit pas avaler les clics du panneau. */
  .logo-img {
    height: 84px;
    width: auto;
    margin-top: -24px;
    margin-bottom: -28px;
    flex-shrink: 0;
    display: block;
    position: relative;
    z-index: 5;
    pointer-events: none;
  }
  /* Topbar : une seule ligne, 3 groupes flex (connexions / live / outils) —
     gap 8px partout. .tb-outils pousse le groupe application à droite. */
  .tb-groupe {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }
  .tb-outils {
    margin-left: auto;
  }
  /* État de connexion : bouton icône de la même famille que Stream (verre
     + inset + rayon global). Gris au repos — la bordure ne prend la
     couleur plateforme (--plat-*) QUE connecté (.on) ; OBS déconnecté
     (.err) et Diffusion en erreur = bordure rouge ; disabled (Diffusion)
     = statut pur, pas de navigation. */
  .indicateur {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    height: 32px;
    padding: 0 0.75rem;
    background: var(--btn-surface);
    box-shadow: var(--btn-inset);
    border: 1px solid var(--bordure-ui-active);
    color: var(--texte);
    font: inherit;
    font-size: 0.8rem;
    cursor: pointer;
    white-space: nowrap;
    flex-shrink: 0;
    transition: background 0.15s ease, border-color 0.15s ease, box-shadow 0.15s ease, opacity 0.15s ease;
  }
  .indicateur:hover:not(:disabled) {
    background: var(--btn-surface-hover);
    border-color: var(--accent-ui);
    box-shadow: var(--btn-inset-hover), var(--glow-ui);
  }
  .indicateur.off {
    color: var(--gris);
    opacity: 0.55;
  }
  /* Icône marque en gris tant que la plateforme est déconnectée — la
     couleur revient avec la bordure à l'état .on. */
  .indicateur.off .ico {
    filter: grayscale(1);
  }
  .indicateur .ico {
    width: 0.95rem;
    height: 0.95rem;
    flex-shrink: 0;
  }
  /* Bordure = couleur plateforme UNIQUEMENT connecté (.on). Déconnecté =
     bordure neutre --bordure-ui-active. .err = couleur d'erreur. */
  .indicateur.plat-twitch.on { border-color: var(--plat-twitch); }
  .indicateur.plat-kick.on { border-color: var(--plat-kick); }
  .indicateur.plat-youtube.on { border-color: var(--plat-youtube); }
  .indicateur.plat-tiktok.on { border-color: var(--plat-tiktok); }
  .indicateur.plat-obs.on { border-color: var(--plat-obs); }
  .indicateur.plat-obs.err,
  .indicateur.plat-diffusion.err {
    opacity: 1;
    border-color: var(--message-error-color);
  }
  .indicateur.plat-diffusion.on {
    border-color: var(--plat-diffusion);
  }
  /* Bouton icône seule permanent 32×32 : Kick/YouTube/TikTok/OBS/Diffusion
     — l'état vit dans la bordure plateforme, le nom dans le title. */
  .indicateur.tb-ico {
    width: 32px;
    padding: 0;
    justify-content: center;
  }
  .indicateur:disabled {
    cursor: default;
  }
  /* Pillule écran d'attente : temps en chiffres tabulaires. */
  .indicateur.plat-attente {
    font-variant-numeric: tabular-nums;
  }
  .indicateur.pause {
    opacity: 0.6;
  }
  .langue-wrap {
    position: relative;
  }
  .langue-btn {
    background: var(--btn-surface);
    box-shadow: var(--btn-inset);
    color: var(--texte);
    border: 1px solid var(--bordure-ui-active);
    height: 32px;
    padding: 0 0.75rem;
    font: inherit;
    font-size: 0.8rem;
    white-space: nowrap;
    flex-shrink: 0;
    cursor: pointer;
    transition: background 0.15s ease, box-shadow 0.15s ease, border-color 0.15s ease;
  }
  .langue-btn:hover {
    background: var(--btn-surface-hover);
    border-color: var(--accent-ui);
    box-shadow: var(--btn-inset-hover), var(--glow-ui);
  }
  /* Libellé de repli topbar (<1200px) : .lg-court = initiale du bouton
     Langue. */
  .lg-court {
    display: none;
  }
  /* Dropdown langue (bouton topbar « Langue ») : même recette visuelle
     que les menus contextuels clic droit (.ctx-menu/.ctx-item, app.css) —
     ancrage absolu sous le bouton à la place du fixed. */
  .langues-dropdown {
    position: absolute;
    top: calc(100% + 4px);
    right: 0;
  }
  .langue-item {
    display: flex;
    align-items: center;
    gap: 0.55rem;
  }
  .langue-nom {
    flex: 1;
  }
  .langue-coche {
    font-weight: 600;
    color: var(--accent-ui);
  }
  /* Boutons contrôle app (stop/restart/refresh OBS) : carrés 32px, icônes
     SVG neutres au repos — la couleur --ctrl-* n'apparaît qu'au survol
     (icône + teinte du verre + bordure + glow assorti), comme sur la
     maquette. Spécificité .ctrl-btn.ctrl-X:hover (0,3,0) > .ctrl-btn:hover
     (0,2,0) → override le glow violet générique. */
  .ctrl-btn {
    background: var(--btn-surface);
    box-shadow: var(--btn-inset);
    color: var(--gris);
    border: 1px solid var(--bordure-ui-active);
    width: 32px;
    height: 32px;
    padding: 0;
    font: inherit;
    line-height: 1;
    cursor: pointer;
    opacity: 0.85;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    transition: background 0.15s ease, box-shadow 0.15s ease, border-color 0.15s ease, color 0.15s ease, opacity 0.15s ease;
  }
  .ctrl-btn .ico {
    width: 0.9rem;
    height: 0.9rem;
  }
  .ctrl-btn:hover {
    background: var(--btn-surface-hover);
    border-color: var(--accent-ui);
    box-shadow: var(--btn-inset-hover), var(--glow-ui);
    color: var(--texte);
    opacity: 1;
  }
  .ctrl-btn.ctrl-arreter:hover {
    --btn-tint: var(--ctrl-arreter);
    color: var(--ctrl-arreter);
    border-color: var(--ctrl-arreter);
    box-shadow: var(--btn-inset-hover), 0 0 8px color-mix(in srgb, var(--ctrl-arreter) 40%, transparent);
  }
  .ctrl-btn.ctrl-redemarrer:hover {
    --btn-tint: var(--ctrl-redemarrer);
    color: var(--ctrl-redemarrer);
    border-color: var(--ctrl-redemarrer);
    box-shadow: var(--btn-inset-hover), var(--glow-orange);
  }
  .ctrl-btn.ctrl-refresh:hover {
    --btn-tint: var(--ctrl-refresh);
    color: var(--ctrl-refresh);
    border-color: var(--ctrl-refresh);
    box-shadow: var(--btn-inset-hover), 0 0 8px color-mix(in srgb, var(--ctrl-refresh) 40%, transparent);
  }
  /* Sorties OBS (stream/record) : recette verre sombre (même base que
     .ctrl-btn) mais proéminente — texte + icône. Vert --message-ok-color au
     repos = « prêt à démarrer » ; rouge --message-error-color quand .actif.
     Surcharge locale de --btn-tint (pattern .speedrun-start CarteEdition). */
  .output-btn {
    --btn-tint: color-mix(in srgb, var(--message-ok-color) 12%, transparent);
    background: var(--btn-surface);
    box-shadow: var(--btn-inset);
    color: var(--message-ok-color);
    border: 1px solid var(--message-ok-color);
    border-radius: var(--rayon-petit);
    width: 32px;
    height: 32px;
    padding: 0;
    font: inherit;
    font-size: 0.8rem;
    line-height: 1;
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    white-space: nowrap;
    flex-shrink: 0;
    transition: background 0.15s ease, box-shadow 0.15s ease, border-color 0.15s ease, color 0.15s ease, opacity 0.15s ease;
  }
  .output-btn .ico {
    width: 0.9rem;
    height: 0.9rem;
  }
  .output-btn:hover:not(:disabled) {
    background: var(--btn-surface-hover);
    box-shadow: var(--btn-inset-hover), 0 0 8px color-mix(in srgb, var(--message-ok-color) 40%, transparent);
  }
  .output-btn.actif {
    --btn-tint: color-mix(in srgb, var(--message-error-color) 12%, transparent);
    color: var(--message-error-color);
    border-color: var(--message-error-color);
    box-shadow: var(--btn-inset), 0 0 8px color-mix(in srgb, var(--message-error-color) 40%, transparent);
  }
  .output-btn.actif:hover:not(:disabled) {
    background: var(--btn-surface-hover);
    box-shadow: var(--btn-inset-hover), 0 0 10px color-mix(in srgb, var(--message-error-color) 50%, transparent);
  }
  /* Grisé neutre (pas de teinte verte/rouge résiduelle) quand OBS déconnecté
     ou toggle en vol. */
  .output-btn:disabled {
    --btn-tint: transparent;
    color: var(--gris);
    border-color: var(--bordure-ui-active);
    box-shadow: var(--btn-inset);
    opacity: 0.5;
    cursor: default;
  }
  /* ===== Responsive topbar (scopé header — ne touche ni sidebar ni
     modales). Une seule ligne à toutes les largeurs : on masque des
     libellés, jamais de wrap. ===== */
  /* <1200px : Exporter/Importer/+ Scène en icône seule, Langue en
     initiale. Seuil volontairement au-dessus des 1080px CSS d'un écran
     portrait (le palier 999px laissait les libellés pleins en portrait →
     les boutons de droite étaient rognés par overflow:clip).
     Paddings resserrés en dernier recours. */
  @media (max-width: 1199px) {
    header {
      padding: 0.35rem 0.5rem;
    }
    .indicateur:not(.tb-ico):not(.tb-action) {
      padding: 0 0.4rem;
    }
    .tb-action {
      width: 32px;
      padding: 0;
      justify-content: center;
    }
    .tb-action .tb-label {
      display: none;
    }
    .langue-btn {
      width: 32px;
      padding: 0;
      text-align: center;
    }
    .langue-btn .lg-full {
      display: none;
    }
    .langue-btn .lg-court {
      display: inline;
    }
  }
  /* <800px : le logo (décoratif, ~90px de boîte) disparaît — l'espace
     récupéré va aux boutons sur les fenêtres vraiment étroites. */
  @media (max-width: 799px) {
    .logo-img {
      display: none;
    }
  }
  .row {
    flex: 1;
    display: flex;
    min-height: 0;
  }
  .sidebar-col {
    display: flex;
    flex-direction: column;
    min-height: 0;
    min-width: 0;
    width: 240px;
    flex-shrink: 0;
    padding: 0.5rem;
    gap: 0.5rem;
    border-right: 1px solid var(--bordure-ui);
    background: transparent; /* laisse transparaître le fond d'ambiance */
  }
  .canvas-col {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }
</style>
