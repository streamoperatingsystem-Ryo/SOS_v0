<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { loadScene } from "./lib/stores/scene";
  import { obsConnect, obsHost, obsPort, obsPassword } from "./lib/stores/obs";
  import { get } from "svelte/store";
  import Toolbar from "./lib/components/Toolbar.svelte";
  import Canvas from "./lib/components/Canvas.svelte";
  import DevPanel from "./lib/components/DevPanel.svelte";

  let serverError: string | null = null;
  let serverOk = false;
  let devOpen = $state(false);

  function toggleDev() {
    devOpen = !devOpen;
  }
  function closeDev() {
    devOpen = false;
  }

  onMount(async () => {
    // Écoute des erreurs du serveur :4321 (bind strict)
    await listen<string>("server_error", (e) => {
      serverError = e.payload;
      serverOk = false;
    });

    // server_ready : serveur :4321 prêt → auto-connect OBS one-shot
    // (défauts 127.0.0.1 / 4455 + mdp UI). Pas de retry en boucle.
    await listen<string>("server_ready", () => {
      serverOk = true;
      serverError = null;
      // Auto-connect OBS : one-shot, si OBS pas lancé → statut error, pas de retry
      obsConnect(get(obsHost), get(obsPort), get(obsPassword));
    });

    // Charge la scène initiale (config.json)
    await loadScene();
  });
</script>

<!-- Clic hors du bloc Dev → ferme le panneau -->
<svelte:window onclick={closeDev} />

<main>
  <header>
    <span class="title">StreamOS v0</span>
    <div class="right">
      <span class="status">
        {#if serverError}
          Erreur :4321 — {serverError}
        {:else if serverOk}
          :4321 prêt
        {:else}
          Démarrage…
        {/if}
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
    <Canvas />
  </div>
</main>

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
  .status {
    opacity: 0.7;
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
</style>
