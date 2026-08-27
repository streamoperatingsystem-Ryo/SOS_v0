<script lang="ts">
  import { createWidget, importMedia, selectedIdStore } from "../stores/scene";
  import { obsConnect, obsStatus, obsError, obsHost, obsPort, obsPassword } from "../stores/obs";
  import { get } from "svelte/store";

  let selectedId = $derived($selectedIdStore);
  let status = $derived($obsStatus);
  let error = $derived($obsError);

  async function onConnect() {
    await obsConnect(get(obsHost), get(obsPort), get(obsPassword));
  }
</script>

<aside class="sidebar">
  <button onclick={createWidget}>Créer un widget</button>
  <button onclick={importMedia} disabled={!selectedId}>Importer une image</button>

  <div class="obs-section">
    <span class="obs-label">OBS</span>
    <input bind:value={$obsHost} placeholder="Hôte" spellcheck="false" />
    <input bind:value={$obsPort} placeholder="Port" spellcheck="false" />
    <input
      bind:value={$obsPassword}
      type="password"
      placeholder="Mot de passe"
      spellcheck="false"
    />
    <button
      onclick={onConnect}
      disabled={status === "connecting"}
    >
      {status === "connecting" ? "Connexion…" : "Connecter"}
    </button>
    <span class="obs-status">
      {#if status === "connected"}
        OBS OK
      {:else if status === "error"}
        {error}
      {/if}
    </span>
  </div>
</aside>

<style>
  .sidebar {
    flex-shrink: 0;
    width: 240px;
    padding: 0.75rem;
    border-right: 1px solid var(--texte);
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    overflow-y: auto;
  }
  button {
    background: var(--fond);
    color: var(--texte);
    border: 1px solid var(--texte);
    padding: 0.4rem 0.8rem;
    font: inherit;
    cursor: pointer;
    text-align: left;
  }
  button:hover {
    background: var(--texte);
    color: var(--fond);
  }
  button:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
  button:disabled:hover {
    background: var(--fond);
    color: var(--texte);
  }
  .obs-section {
    margin-top: 1rem;
    padding-top: 0.75rem;
    border-top: 1px solid var(--texte);
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
  }
  .obs-label {
    font-size: 0.8rem;
    opacity: 0.7;
    margin-bottom: 0.1rem;
  }
  input {
    background: var(--fond);
    color: var(--texte);
    border: 1px solid var(--texte);
    padding: 0.35rem 0.5rem;
    font: inherit;
    font-size: 0.85rem;
    width: 100%;
  }
  input:focus {
    outline: none;
    border-color: var(--texte);
    background: rgba(224, 224, 224, 0.05);
  }
  .obs-status {
    font-size: 0.8rem;
    opacity: 0.7;
    word-break: break-word;
  }
</style>
