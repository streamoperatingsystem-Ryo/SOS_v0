<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { loadScene, selectedIdStore, loadedStore } from "./lib/stores/scene";
  import { loadScenesIndex, loadCurrentScene } from "./lib/stores/scenes";
  import { confirmDeleteWidget, cadreModalOpen, interactionModalOpen, moderationModalOpen, titreFondModalOpen, speedrunConfigModalOpen } from "./lib/stores/ui";
  import { obsConnect, obsStatus, obsError, obsHost, obsPort, obsPassword } from "./lib/stores/obs";
  import { initChat, twitchDevice, twitchLogin, youtubeDevice, youtubeLogin, kickSlug, tiktokUsername, connexions } from "./lib/stores/chat";
  import { chargerCommunaute, resetCommunaute, chargerCommunauteYoutube } from "./lib/stores/communaute";
  import { initSpeedrun, chargerPreferencesSpeedrun } from "./lib/stores/speedrun";
  import { tauri } from "./lib/tauri";
  import { injecterFontsCSS } from "./lib/fonts";
  import { get } from "svelte/store";
  import Toolbar from "./lib/components/Toolbar.svelte";
  import CarteEdition from "./lib/components/CarteEdition.svelte";
  import Canvas from "./lib/components/Canvas.svelte";
  import SceneBar from "./lib/components/SceneBar.svelte";
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

  // ===== Communauté : chargement réactif =====
  // $effect réactif : charge la communauté quand Twitch passe connecté,
  // reset quand déconnecté. Plus fiable qu'un listener d'event async
  // (pas de race condition avec l'enregistrement). Vit dans App.svelte
  // (toujours monté) pour précharger les données même si la modale
  // Modération & Rôles n'est pas ouverte (la section Viewers en dépend).
  $effect(() => {
    if (cx.twitch) {
      console.log("[Communauté] twitch connecté → chargement");
      chargerCommunaute();
    } else {
      console.log("[Communauté] twitch déconnecté → reset");
      resetCommunaute();
    }
  });

  // $effect YouTube : charge la communauté YouTube quand connecté.
  $effect(() => {
    if (cx.youtube) {
      console.log("[Communauté] youtube connecté → chargement");
      chargerCommunauteYoutube();
    }
  });

  function toggleDev() {
    devOpen = !devOpen;
  }
  function closeDev() {
    devOpen = false;
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
      console.log("[Refresh] appel obsRefreshDiffusion…");
      await tauri.obsRefreshDiffusion(
        get(obsHost), parseInt(get(obsPort), 10), get(obsPassword)
      );
      console.log("[Refresh] obsRefreshDiffusion OK");
    } catch (e) {
      console.warn("[Refresh] obsRefreshDiffusion échoué:", e);
    }
  }

  // Auto-connect OBS + refresh SOS-Diffusion + sync captures.
  // Appelé quand serveur :4321 est prêt (event ou fetch fallback).
  // Le sync captures APRÈS obsConnect garantit : SOS-Trou-* actifs + source
  // "SOS-Caméra" du widget caméra (auto-guérison si créée sans OBS). Corrige
  // aussi la race boot : le sync du onMount (step 6) peut s'exécuter avant la
  // fin de obsConnect → ici on est sûr que OBS est connecté.
  async function bootObs() {
    console.log("[Boot] bootObs → obsConnect");
    try {
      await obsConnect(get(obsHost), get(obsPort), get(obsPassword));
      // obsConnect a mis obsStatus=connected. Refresh direct (pas de race).
      await refreshDiffusion();
      // Sync captures : non-fatal (OBS offline ou scène SOS absente → skip).
      try {
        await tauri.sceneSyncCaptures(
          get(obsHost), parseInt(get(obsPort), 10), get(obsPassword)
        );
      } catch (e) {
        console.warn("[Boot] sync captures:", e);
      }
    } catch (e) {
      console.warn("[Boot] bootObs obsConnect échoué:", e);
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
      console.log("[Boot] server_ready event → bootObs");
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

    // 4. Fallback : si :4321 déjà up (event manqué), boot OBS maintenant.
    const up = await checkServerUp();
    if (up && !serverOk) {
      serverOk = true;
      serverError = null;
      console.log("[Boot] fetch fallback :4321 up → bootObs");
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
        console.log("[Boot] auto-reconnect OBS (non connecté)…");
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
  <header>
    <span class="title">StreamOS <span class="v0">v0</span></span>
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
        <button class="ctrl-btn ctrl-arreter" onclick={onArreter} title="Arrêter l'application">■</button>
        <button class="ctrl-btn ctrl-redemarrer" onclick={onRedemarrer} title="Redémarrer l'application">↻</button>
        <button class="ctrl-btn ctrl-refresh" onclick={onRefreshObs} title="Refresh OBS / SOS-Diffusion">⟳</button>
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
    border-bottom: 1px solid var(--bordure);
    background: var(--fond-panneau);
    font-size: 0.85rem;
  }
  /* Logo « StreamOS v0 » : dégradé signature sur « StreamOS » (sans asset image),
     « v0 » en gris secondaire. */
  .title {
    font-weight: 700;
    background: var(--gradient-sig);
    -webkit-background-clip: text;
    background-clip: text;
    color: transparent;
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
    background: var(--bordure-active);
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
  /* Pastille statut : hors-ligne = gris neutre (décoratif, pas rouge erreur).
     Connectée = couleur de marque de la plateforme (--plat-*). OBS/Diffusion
     gardent le vert sémantique (--message-ok-color) via .plat-obs/.plat-diffusion. */
  .dot {
    display: inline-block;
    width: 0.5rem;
    height: 0.5rem;
    border-radius: 50%;
    background: var(--gris);
    flex-shrink: 0;
    transition: background-color 0.15s ease, box-shadow 0.15s ease;
  }
  .dot.on {
    background: var(--message-ok-color);
  }
  .dot.plat-twitch.on { background: var(--plat-twitch); box-shadow: 0 0 6px rgba(168, 85, 247, 0.6); }
  .dot.plat-kick.on { background: var(--plat-kick); box-shadow: 0 0 6px rgba(34, 197, 94, 0.6); }
  .dot.plat-youtube.on { background: var(--plat-youtube); box-shadow: 0 0 6px rgba(244, 63, 94, 0.6); }
  .dot.plat-tiktok.on { background: var(--plat-tiktok); box-shadow: 0 0 6px rgba(236, 72, 153, 0.6); }
  .dot.plat-obs.on { background: var(--plat-obs); }
  .dot.plat-diffusion.on { background: var(--plat-diffusion); box-shadow: 0 0 6px rgba(34, 197, 94, 0.6); }
  .dev-wrap {
    position: relative;
  }
  .dev-btn {
    --btn-tint: var(--accent-violet);
    background: var(--btn-surface);
    box-shadow: var(--btn-inset);
    color: var(--texte);
    border: 1px solid var(--bordure);
    padding: 0.15rem 0.5rem;
    font: inherit;
    font-size: 0.8rem;
    cursor: pointer;
    transition: background 0.15s ease, box-shadow 0.15s ease, border-color 0.15s ease;
  }
  .dev-btn:hover {
    background: var(--btn-surface-hover);
    border-color: var(--accent-violet);
    box-shadow: var(--btn-inset-hover), var(--glow-violet);
  }
  .ctrl-buttons {
    display: flex;
    align-items: center;
    gap: 0.25rem;
  }
  .ctrl-btn {
    --btn-tint: var(--accent-violet);
    background: var(--btn-surface);
    box-shadow: var(--btn-inset);
    color: var(--texte);
    border: 1px solid var(--bordure);
    padding: 0.1rem 0.4rem;
    font: inherit;
    font-size: 0.85rem;
    line-height: 1;
    cursor: pointer;
    opacity: 0.7;
    transition: background 0.15s ease, box-shadow 0.15s ease, border-color 0.15s ease, opacity 0.15s ease;
  }
  .ctrl-btn:hover {
    background: var(--btn-surface-hover);
    border-color: var(--accent-violet);
    box-shadow: var(--btn-inset-hover), var(--glow-violet);
    opacity: 1;
  }
  /* Arrêter : rouge danger (quitte l'app). La teinte colore le verre +
     la bordure au survol. Spécificité .ctrl-btn.ctrl-arreter (0,3,0) >
     .ctrl-btn:hover (0,2,0) → override le glow violet générique. */
  .ctrl-btn.ctrl-arreter {
    --btn-tint: var(--ctrl-arreter);
    color: var(--ctrl-arreter);
  }
  .ctrl-btn.ctrl-arreter:hover {
    border-color: var(--ctrl-arreter);
    box-shadow: var(--btn-inset-hover), 0 0 8px rgba(192, 57, 43, 0.4);
  }
  /* Redémarrer : orange action utilisateur (relance). */
  .ctrl-btn.ctrl-redemarrer {
    --btn-tint: var(--ctrl-redemarrer);
    color: var(--ctrl-redemarrer);
  }
  .ctrl-btn.ctrl-redemarrer:hover {
    border-color: var(--ctrl-redemarrer);
    box-shadow: var(--btn-inset-hover), var(--glow-orange);
  }
  /* Refresh OBS : vert succès (refresh diffusion). */
  .ctrl-btn.ctrl-refresh {
    --btn-tint: var(--ctrl-refresh);
    color: var(--ctrl-refresh);
  }
  .ctrl-btn.ctrl-refresh:hover {
    border-color: var(--ctrl-refresh);
    box-shadow: var(--btn-inset-hover), 0 0 8px rgba(34, 197, 94, 0.4);
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
    border-right: 1px solid var(--bordure);
    background: var(--fond-panneau);
  }
  .canvas-col {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }
</style>
