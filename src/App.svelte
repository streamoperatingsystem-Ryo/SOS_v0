<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { loadScene, selectedIdStore, loadedStore } from "./lib/stores/scene";
  import { loadScenesIndex, loadCurrentScene } from "./lib/stores/scenes";
  import { confirmDeleteWidget, cadreModalOpen } from "./lib/stores/ui";
  import { obsConnect, obsStatus, obsHost, obsPort, obsPassword } from "./lib/stores/obs";
  import { initChat, twitchDevice, twitchLogin, youtubeDevice, youtubeLogin, kickSlug, tiktokUsername, connexions } from "./lib/stores/chat";
  import { tauri } from "./lib/tauri";
  import { get } from "svelte/store";
  import Toolbar from "./lib/components/Toolbar.svelte";
  import Canvas from "./lib/components/Canvas.svelte";
  import SceneBar from "./lib/components/SceneBar.svelte";
  import DevPanel from "./lib/components/DevPanel.svelte";
  import TwitchDeviceModal from "./lib/components/TwitchDeviceModal.svelte";
  import ConfirmDeleteWidgetModal from "./lib/components/ConfirmDeleteWidgetModal.svelte";
  import CadresModal from "./lib/components/CadresModal.svelte";
  import WelcomeCommunauteModal from "./lib/components/WelcomeCommunauteModal.svelte";
  import WelcomeQueuePanel from "./lib/components/WelcomeQueuePanel.svelte";
  import { initWelcome, chargerWelcome } from "./lib/stores/welcome";
  let welcomeModalOpen = $state(false);

  let serverError = $state<string | null>(null);
  let serverOk = $state(false);
  let devOpen = $state(false);

  // Track OBS connecté pour refresh auto SOS-Diffusion.
  // Refresh quand server ready + OBS connecté (boot + reconnect).
  let obsConnected = $state(false);

  // États dérivés pour le bandeau unique (Twitch / OBS / :4321).
  let cx = $derived($connexions);
  let login = $derived($twitchLogin);
  let ytLogin = $derived($youtubeLogin);
  let kickSl = $derived($kickSlug);
  let ttUser = $derived($tiktokUsername);
  let obsSt = $derived($obsStatus);

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

  // Auto-connect OBS + refresh SOS-Diffusion.
  // Appelé quand serveur :4321 est prêt (event ou fetch fallback).
  async function bootObs() {
    console.log("[Boot] bootObs → obsConnect");
    try {
      await obsConnect(get(obsHost), get(obsPort), get(obsPassword));
      // obsConnect a mis obsStatus=connected. Refresh direct (pas de race).
      await refreshDiffusion();
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

    // 2. Subscribe obsStatus pour le reconnect manuel (clic Connecter).
    //    Le boot est géré par bootObs() ci-dessus (pas de race ici).
    const unsubObs = obsStatus.subscribe((s) => {
      obsConnected = s === "connected";
      console.log("[Boot] obsStatus=" + s);
    });

    // 3. initChat (listeners twitch:connecte, chat:message, etc.)
    await initChat();

    // 3b. initWelcome (listeners welcome:etat + welcome:attribution-progress)
    //     + charge l'état initial de la queue + registre.
    await initWelcome();
    await chargerWelcome();

    // 4. Fallback : si :4321 déjà up (event manqué), boot OBS maintenant.
    const up = await checkServerUp();
    if (up && !serverOk) {
      serverOk = true;
      serverError = null;
      console.log("[Boot] fetch fallback :4321 up → bootObs");
      bootObs();
    }

    // 5. Charge la scène initiale + index scènes + scène courante
    await loadScene();
    await loadScenesIndex();
    await loadCurrentScene();

    // 6. Sync captures OBS au boot (activer SOS-Trou-* de la scène courante).
    //    Non-fatal si OBS offline (le refresh se fera au prochain openScene).
    try {
      await tauri.sceneSyncCaptures(
        get(obsHost), parseInt(get(obsPort), 10), get(obsPassword)
      );
    } catch (e) {
      console.warn("[Boot] sync captures OBS échoué:", e);
    }

    return () => unsubObs();
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
    <span class="title">StreamOS v0</span>
    <div class="right">
      <!-- Bandeau unique : Twitch / Kick / YouTube / TikTok / OBS / :4321 (lecture seule) -->
      <span class="bandeau">
        <span class="indicateur" class:on={cx.twitch} class:off={!cx.twitch}>
          <span class="dot" class:on={cx.twitch} class:off={!cx.twitch}></span>
          Twitch{#if cx.twitch && login} : {login}{/if}
        </span>
        <span class="bandeau-sep"></span>
        <span class="indicateur" class:on={cx.kick} class:off={!cx.kick}>
          <span class="dot" class:on={cx.kick} class:off={!cx.kick}></span>
          Kick{#if cx.kick && kickSl} : {kickSl}{/if}
        </span>
        <span class="bandeau-sep"></span>
        <span class="indicateur" class:on={cx.youtube} class:off={!cx.youtube}>
          <span class="dot" class:on={cx.youtube} class:off={!cx.youtube}></span>
          YouTube{#if cx.youtube && ytLogin} : {ytLogin}{/if}
        </span>
        <span class="bandeau-sep"></span>
        <span class="indicateur" class:on={cx.tiktok} class:off={!cx.tiktok}>
          <span class="dot" class:on={cx.tiktok} class:off={!cx.tiktok}></span>
          TikTok{#if cx.tiktok && ttUser} : {ttUser}{/if}
        </span>
        <span class="bandeau-sep"></span>
        <span class="indicateur" class:on={obsSt === "connected"} class:off={obsSt !== "connected"}>
          <span class="dot" class:on={obsSt === "connected"} class:off={obsSt !== "connected"}></span>
          OBS{#if obsSt === "connected"} OK{:else if obsSt === "error"} Err{/if}
        </span>
        <span class="bandeau-sep"></span>
        <span class="indicateur" class:on={serverOk} class:off={!serverOk}>
          <span class="dot" class:on={serverOk} class:off={!serverOk}></span>
          :4321{#if serverError} Err{:else if serverOk} prêt{:else} …{/if}
        </span>
      </span>
      <!-- Boutons contrôle : Arrêter / Redémarrer / Refresh OBS -->
      <span class="bandeau-sep"></span>
      <div class="ctrl-buttons">
        <button class="ctrl-btn" onclick={onArreter} title="Arrêter l'application">■</button>
        <button class="ctrl-btn" onclick={onRedemarrer} title="Redémarrer l'application">↻</button>
        <button class="ctrl-btn" onclick={onRefreshObs} title="Refresh OBS / SOS-Diffusion">⟳</button>
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
    <Toolbar />
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

<!-- Modale attribution clips de bienvenue (followers + clips) -->
{#if welcomeModalOpen}
  <WelcomeCommunauteModal onFermer={() => (welcomeModalOpen = false)} />
{/if}

<!-- Panel file d'attente clips de bienvenue (toujours visible, repliable) -->
<WelcomeQueuePanel />

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
    border-bottom: 1px solid var(--texte);
    font-size: 0.85rem;
  }
  .title {
    font-weight: 600;
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
    background: var(--texte);
    opacity: 0.25;
    flex-shrink: 0;
  }
  .indicateur {
    display: inline-flex;
    align-items: center;
    gap: 0.25rem;
    opacity: 0.85;
  }
  .indicateur.off {
    opacity: 0.5;
  }
  .dot {
    display: inline-block;
    width: 0.5rem;
    height: 0.5rem;
    border-radius: 50%;
    background: #c33;
    flex-shrink: 0;
  }
  .dot.on {
    background: #3c3;
  }
  .dev-wrap {
    position: relative;
  }
  .dev-btn {
    background: var(--fond);
    color: var(--texte);
    border: 1px solid var(--texte);
    padding: 0.15rem 0.5rem;
    font: inherit;
    font-size: 0.8rem;
    cursor: pointer;
  }
  .dev-btn:hover {
    background: var(--texte);
    color: var(--fond);
  }
  .ctrl-buttons {
    display: flex;
    align-items: center;
    gap: 0.25rem;
  }
  .ctrl-btn {
    background: var(--fond);
    color: var(--texte);
    border: 1px solid var(--texte);
    padding: 0.1rem 0.4rem;
    font: inherit;
    font-size: 0.85rem;
    line-height: 1;
    cursor: pointer;
    opacity: 0.7;
  }
  .ctrl-btn:hover {
    background: var(--texte);
    color: var(--fond);
    opacity: 1;
  }
  .row {
    flex: 1;
    display: flex;
    min-height: 0;
  }
  .canvas-col {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }
</style>
