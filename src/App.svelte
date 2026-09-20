<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { listen, emitTo } from "@tauri-apps/api/event";
  import { loadScene, selectedIdStore, loadedStore, createWidget, createChatWidget, createCameraWidget, createInputViewerWidget, createSpeedrunWidget, sceneStore } from "./lib/stores/scene";
  import { loadScenesIndex, loadCurrentScene } from "./lib/stores/scenes";
  import { confirmDeleteWidget, cadreModalOpen, interactionModalOpen, moderationModalOpen, titreFondModalOpen, speedrunConfigModalOpen, openSectionExplicit } from "./lib/stores/ui";
  import { obsConnect, obsStatus, obsError, obsHost, obsPort, obsPassword, obsStreaming, obsRecording, toggleStreamObs, toggleRecordObs } from "./lib/stores/obs";
  import { initChat, twitchDevice, twitchLogin, youtubeDevice, youtubeLogin, kickSlug, tiktokUsername, connexions, connecterTwitch, deconnecterTwitch } from "./lib/stores/chat";
  import { connecterKick, deconnecterKick, lireSlugSauve } from "./lib/stores/kick";
  import { connecterYoutube, deconnecterYoutube } from "./lib/stores/youtube";
  import { connecterTiktok, deconnecterTiktok, lireUsernameSauve } from "./lib/stores/tiktok";
  import { initRaccourcis, masquerRaccourcis } from "./lib/stores/raccourcis";
  import { chargerCommunaute, resetCommunaute, chargerCommunauteYoutube } from "./lib/stores/communaute";
  import { initSpeedrun, chargerPreferencesSpeedrun } from "./lib/stores/speedrun";
  import { tauri } from "./lib/tauri";
  import { injecterFontsCSS } from "./lib/fonts";
  import { get } from "svelte/store";
  import Toolbar from "./lib/components/Toolbar.svelte";
  import CarteEdition from "./lib/components/CarteEdition.svelte";
  import Canvas from "./lib/components/Canvas.svelte";
  import SceneBar from "./lib/components/SceneBar.svelte";
  import DropBar from "./lib/components/DropBar.svelte";
  import DevPanel from "./lib/components/DevPanel.svelte";
  import TwitchDeviceModal from "./lib/components/TwitchDeviceModal.svelte";
  import ConfirmDeleteWidgetModal from "./lib/components/ConfirmDeleteWidgetModal.svelte";
  import CadresModal from "./lib/components/CadresModal.svelte";
  import InteractionViewerModal from "./lib/components/InteractionViewerModal.svelte";
  import ModerationModal from "./lib/components/ModerationModal.svelte";
  import TitreModal from "./lib/components/TitreModal.svelte";
  import TitreFondModal from "./lib/components/TitreFondModal.svelte";
  import SpeedrunConfigModal from "./lib/components/SpeedrunConfigModal.svelte";
  import { initWelcome, chargerWelcome } from "./lib/stores/welcome";
  import { initBandeau, chargerBandeau } from "./lib/stores/bandeau";
  import { chargerAlertes, demarrerDiffFollows } from "./lib/stores/alertes";
  import { initPositionOverlay, chargerPositionOverlay } from "./lib/stores/positionOverlay";
  import { initPad, chargerPad } from "./lib/stores/padNumerique";
  import { fondSidebar } from "./lib/stores/fondSidebar";
  import { gradientCadre } from "./lib/cadres/registre";

  let serverError = $state<string | null>(null);
  let serverOk = $state(false);
  let devOpen = $state(false);
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

  // Couleur EFFECTIVE du cadre SVG → tout le chrome la suit : poussée dans
  // --cadre-couleur / --cadre-couleur-fin sur :root, consommée par
  // --accent-ui (bordures/hovers/focus), --fond-ambiance, .fond-decor et
  // le bouton « Vos scènes ». Résolue via gradientCadre() — le même
  // resolver que le rendu : cadre personnalisable → couleur/couleurFin ;
  // preset → palette de la variante (le champ couleur n'y est PAS la
  // source de vérité). Fallback cadreWidget puis accents du thème.
  // Image de fond du dashboard (Personnalisation → « Image de fond ») —
  // servie par :4321 comme les médias de scène, sous tout le décor.
  let fondSidebarUrl = $derived($fondSidebar ? "http://127.0.0.1:4321/" + $fondSidebar : null);

  let cadreActif = $derived(
    $sceneStore.cadreApp?.actif
      ? $sceneStore.cadreApp
      : $sceneStore.cadreWidget?.actif
        ? $sceneStore.cadreWidget
        : null
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

  function toggleDev() {
    devOpen = !devOpen;
  }
  function closeDev() {
    devOpen = false;
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
          await tauri.sceneSyncCaptures(
            get(obsHost), parseInt(get(obsPort), 10), get(obsPassword)
          );
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
      case "modale-cadres": cadreModalOpen.set("widget"); break;
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
    }).catch(() => {});
  }

  $effect(() => {
    // Référence réactive : relire les stores à chaque changement.
    void cx;
    void obsSt;
    void obsStream;
    void obsRecord;
    pushRaccourcisConnexions();
  });

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
      // Sync captures : non-fatal (OBS offline ou scène SOS absente → skip).
      try {
        await tauri.sceneSyncCaptures(
          get(obsHost), parseInt(get(obsPort), 10), get(obsPassword)
        );
      } catch {
      }
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

    // 3b. initWelcome (listeners welcome:etat + welcome:attribution-progress)
    //     + charge l'état initial de la queue + registre.
    await initWelcome();
    await chargerWelcome();

    // 3c. initBandeau (listener bandeau:etat) + charge l'état initial.
    await initBandeau();
    await chargerBandeau();

    // 3d. Alertes : charge la config + démarre la diff des follows (polling
    //     Helix 60s — baseline au 1er poll, alertes sur les nouveaux ensuite).
    await chargerAlertes();
    demarrerDiffFollows();

    // 3e. Speedrun Splitter : écoute l'event speedrun:event + charge les
    //     préférences sauvegardées (chemins ASL/LSS + settings).
    await initSpeedrun();
    await chargerPreferencesSpeedrun();

    // 3e. Squelette de position unifié (clip de bienvenue + alertes) :
    //     init listener + charge la config initiale depuis Rust.
    await initPositionOverlay();
    await chargerPositionOverlay();

    // 3f. Pad numérique (16 touches Numpad × 3 plages → overlay diffusion) :
    //     init listeners (pad:etat, pad:touche, pad:touche-stop, pad:plage-changee)
    //     + charge la config initiale. L'audio local est joué côté dashboard.
    await initPad();
    await chargerPad();

    // 3g. Barre de raccourcis : état initial (visible/bord/moniteur) + listener
    //     des actions émises par la fenêtre + handshake "ready" (la barre peut
    //     être créée avant que le dashboard ait fini son init → elle demande
    //     l'état des connexions pour ses pastilles).
    await initRaccourcis();
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

<!-- Clic hors du bloc Dev → ferme le panneau.
     onkeydown : touche Suppr → confirmation suppression widget (handler onSupprKey). -->
<svelte:window onclick={closeDev} onkeydown={onSupprKey} />

<main>
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
    <span class="title">
      <svg class="logo" viewBox="0 0 24 24" aria-hidden="true">
        <defs>
          <linearGradient id="logo-grad" x1="0" y1="0" x2="1" y2="1">
            <stop offset="0" style="stop-color: var(--accent-orange)" />
            <stop offset="0.5" style="stop-color: var(--accent-magenta)" />
            <stop offset="1" style="stop-color: var(--accent-violet)" />
          </linearGradient>
        </defs>
        <rect x="1.6" y="1.6" width="20.8" height="20.8" rx="6" fill="none" stroke="url(#logo-grad)" stroke-width="1.7" />
        <path d="M9.5 7.4v9.2l7.6-4.6z" fill="url(#logo-grad)" />
      </svg>
      StreamOS <span class="v0">v0</span>
    </span>
    <div class="right">
      <!-- Bandeau unique : Twitch / Kick / YouTube / TikTok / OBS / Diffusion (lecture seule) -->
      <span class="bandeau">
        <span class="indicateur plat-twitch" class:on={cx.twitch} class:off={!cx.twitch}>
          <span class="dot plat-twitch" class:on={cx.twitch} class:off={!cx.twitch}></span>
          Twitch{#if cx.twitch && login} : {login}{/if}
        </span>
        <span class="bandeau-sep"></span>
        <span class="indicateur plat-kick" class:on={cx.kick} class:off={!cx.kick}>
          <span class="dot plat-kick" class:on={cx.kick} class:off={!cx.kick}></span>
          Kick{#if cx.kick && kickSl} : {kickSl}{/if}
        </span>
        <span class="bandeau-sep"></span>
        <span class="indicateur plat-youtube" class:on={cx.youtube} class:off={!cx.youtube}>
          <span class="dot plat-youtube" class:on={cx.youtube} class:off={!cx.youtube}></span>
          YouTube{#if cx.youtube && ytLogin} : {ytLogin}{/if}
        </span>
        <span class="bandeau-sep"></span>
        <span class="indicateur plat-tiktok" class:on={cx.tiktok} class:off={!cx.tiktok}>
          <span class="dot plat-tiktok" class:on={cx.tiktok} class:off={!cx.tiktok}></span>
          TikTok{#if cx.tiktok && ttUser} : {ttUser}{/if}
        </span>
        <span class="bandeau-sep"></span>
        <span class="bandeau-sep"></span>
        <span
          class="indicateur plat-obs"
          class:on={obsSt === "connected"}
          class:off={obsSt !== "connected"}
          title={obsErr ?? "OBS WebSocket (voir section OBS de la barre latérale)"}
        >
          <span class="dot plat-obs" class:on={obsSt === "connected"} class:off={obsSt !== "connected"}></span>
          {#if obsSt === "connected"}
            OBS
          {:else if obsSt === "connecting"}
            OBS (connexion…)
          {:else}
            OBS (non connecté)
          {/if}
        </span>
        <span class="bandeau-sep"></span>
        <span
          class="indicateur plat-diffusion"
          class:on={serverOk}
          class:off={!serverOk}
          title={serverError ?? "Serveur de diffusion local (consommé par OBS comme source navigateur)"}
        >
          <span class="dot plat-diffusion" class:on={serverOk} class:off={!serverOk}></span>
          {#if serverError}
            Diffusion (erreur)
          {:else if serverOk}
            Diffusion (prête)
          {:else}
            Diffusion (démarrage…)
          {/if}
        </span>
      </span>
      <!-- Boutons contrôle : Arrêter / Redémarrer / Refresh OBS -->
      <span class="bandeau-sep"></span>
      <div class="ctrl-buttons">
        <button class="ctrl-btn ctrl-arreter" onclick={onArreter} title="Arrêter l'application" aria-label="Arrêter l'application">
          <svg class="ico" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><rect x="7" y="7" width="10" height="10" rx="1.5"/></svg>
        </button>
        <button class="ctrl-btn ctrl-redemarrer" onclick={onRedemarrer} title="Redémarrer l'application" aria-label="Redémarrer l'application">
          <svg class="ico" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M21 12a9 9 0 1 1-2.64-6.36"/><path d="M21 3v5h-5"/></svg>
        </button>
        <button class="ctrl-btn ctrl-refresh" onclick={onRefreshObs} title="Refresh OBS / SOS-Diffusion" aria-label="Refresh OBS / SOS-Diffusion">
          <svg class="ico" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8"/><path d="M21 3v5h-5"/><path d="M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16"/><path d="M3 21v-5h5"/></svg>
        </button>
      </div>
      <!-- TEMPORAIRE — bouton Dev. Retirer avant release. -->
      <div
        class="dev-wrap"
        role="presentation"
        onclick={(e) => e.stopPropagation()}
        onkeydown={(e) => e.stopPropagation()}
      >
        <button class="dev-btn" onclick={toggleDev}>Dev</button>
        {#if devOpen}
          <DevPanel />
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
      <SceneBar />
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
  <CadresModal mode={$cadreModalOpen} />
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

<style>
  main {
    display: flex;
    flex-direction: column;
    height: 100%;
  }
  header {
    flex-shrink: 0;
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0.35rem 0.75rem;
    border-bottom: 1px solid var(--bordure-ui);
    background: transparent; /* laisse transparaître le fond d'ambiance */
    font-size: 0.85rem;
  }
  /* Logo « StreamOS v0 » : marque SVG (carré arrondi + play) en dégradé
     signature + « StreamOS » dégradé, « v0 » en gris secondaire. */
  .title {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    font-weight: 700;
    background: var(--gradient-sig);
    -webkit-background-clip: text;
    background-clip: text;
    color: transparent;
  }
  .title .logo {
    width: 1.05rem;
    height: 1.05rem;
    flex-shrink: 0;
  }
  .title .v0 {
    background: none;
    -webkit-text-fill-color: var(--gris);
    color: var(--gris);
    font-weight: 400;
  }
  .right {
    display: flex;
    align-items: center;
    gap: 0.6rem;
  }
  .bandeau {
    display: flex;
    align-items: center;
    gap: 0.7rem;
    font-size: 0.8rem;
  }
  .bandeau-sep {
    width: 1px;
    height: 0.9rem;
    background: var(--bordure-ui-active);
    opacity: 0.6;
    flex-shrink: 0;
  }
  .indicateur {
    display: inline-flex;
    align-items: center;
    gap: 0.25rem;
    opacity: 0.85;
  }
  .indicateur.off {
    opacity: 0.4;
    color: var(--gris);
  }
  /* .dot (pastille statut) : classe partagée définie dans app.css. */
  .dev-wrap {
    position: relative;
  }
  .dev-btn {
    background: var(--btn-surface);
    box-shadow: var(--btn-inset);
    color: var(--texte);
    border: 1px solid var(--bordure-ui-active);
    padding: 0.15rem 0.5rem;
    font: inherit;
    font-size: 0.8rem;
    cursor: pointer;
    transition: background 0.15s ease, box-shadow 0.15s ease, border-color 0.15s ease;
  }
  .dev-btn:hover {
    background: var(--btn-surface-hover);
    border-color: var(--accent-ui);
    box-shadow: var(--btn-inset-hover), var(--glow-ui);
  }
  .ctrl-buttons {
    display: flex;
    align-items: center;
    gap: 0.25rem;
  }
  /* Boutons contrôle app (stop/restart/refresh OBS) : icônes SVG neutres au
     repos — la couleur --ctrl-* n'apparaît qu'au survol (icône + teinte du
     verre + bordure + glow assorti), comme sur la maquette. Spécificité
     .ctrl-btn.ctrl-X:hover (0,3,0) > .ctrl-btn:hover (0,2,0) → override le
     glow violet générique. */
  .ctrl-btn {
    background: var(--btn-surface);
    box-shadow: var(--btn-inset);
    color: var(--gris);
    border: 1px solid var(--bordure-ui-active);
    padding: 0.15rem 0.45rem;
    font: inherit;
    line-height: 1;
    cursor: pointer;
    opacity: 0.85;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    transition: background 0.15s ease, box-shadow 0.15s ease, border-color 0.15s ease, color 0.15s ease, opacity 0.15s ease;
  }
  .ctrl-btn .ico {
    width: 0.8rem;
    height: 0.8rem;
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
