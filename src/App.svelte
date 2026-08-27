<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { loadScene } from "./lib/stores/scene";
  import { obsConnect, obsHost, obsPort, obsPassword } from "./lib/stores/obs";
  import { get } from "svelte/store";
  import Toolbar from "./lib/components/Toolbar.svelte";
  import Canvas from "./lib/components/Canvas.svelte";

  let serverError: string | null = null;
  let serverOk = false;

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

<main>
  <header>
    <span class="title">StreamOS v0</span>
    <span class="status">
      {#if serverError}
        Erreur :4321 — {serverError}
      {:else if serverOk}
        :4321 prêt
      {:else}
        Démarrage…
      {/if}
    </span>
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
  .status {
    opacity: 0.7;
  }
  .row {
    flex: 1;
    display: flex;
    min-height: 0;
  }
</style>
