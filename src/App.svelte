<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { loadScene } from "./lib/stores/scene";
  import { loadScenesIndex, loadCurrentScene } from "./lib/stores/scenes";
  import { obsConnect, obsStatus, obsHost, obsPort, obsPassword } from "./lib/stores/obs";
  import { initChat, twitchDevice, twitchLogin, connexions } from "./lib/stores/chat";
  import { tauri } from "./lib/tauri";
  import { get } from "svelte/store";
  import Toolbar from "./lib/components/Toolbar.svelte";
  import Canvas from "./lib/components/Canvas.svelte";
  import SceneBar from "./lib/components/SceneBar.svelte";
  import DevPanel from "./lib/components/DevPanel.svelte";
  import TwitchDeviceModal from "./lib/components/TwitchDeviceModal.svelte";

  let serverError = $state<string | null>(null);
  let serverOk = $state(false);
  let devOpen = $state(false);

  // Track OBS connecté pour refresh auto SOS-Diffusion.
  // Refresh quand server ready + OBS connecté (boot + reconnect).
  let obsConnected = $state(false);

  // États dérivés pour le bandeau unique (Twitch / OBS / :4321).
  let cx = $derived($connexions);
  let login = $derived($twitchLogin);
  let obsSt = $derived($obsStatus);

  function toggleDev() {
    devOpen = !devOpen;
  }
  function closeDev() {
    devOpen = false;
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
</script>

<!-- Clic hors du bloc Dev → ferme le panneau -->
<svelte:window onclick={closeDev} />

<main>
  <header>
    <span class="title">StreamOS v0</span>
    <div class="right">
      <!-- Bandeau unique : Twitch / OBS / :4321 (lecture seule) -->
      <span class="bandeau">
        <span class="indicateur" class:on={cx.twitch} class:off={!cx.twitch}>
          <span class="dot" class:on={cx.twitch} class:off={!cx.twitch}></span>
          Twitch{#if cx.twitch && login} : {login}{/if}
        </span>
        <span class="indicateur" class:on={obsSt === "connected"} class:off={obsSt !== "connected"}>
          <span class="dot" class:on={obsSt === "connected"} class:off={obsSt !== "connected"}></span>
          OBS{#if obsSt === "connected"} OK{:else if obsSt === "error"} Err{/if}
        </span>
        <span class="indicateur" class:on={serverOk} class:off={!serverOk}>
          <span class="dot" class:on={serverOk} class:off={!serverOk}></span>
          :4321{#if serverError} Err{:else if serverOk} prêt{:else} …{/if}
        </span>
      </span>
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

{#if $twitchDevice}
  <TwitchDeviceModal />
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
