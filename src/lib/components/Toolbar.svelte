<script lang="ts">
  import { createWidget, importMedia, setMediaFit, selectedIdStore, sceneStore } from "../stores/scene";
  import { obsConnect, obsStatus, obsError, obsHost, obsPort, obsPassword } from "../stores/obs";
  import { openSection, toggleSection } from "../stores/ui";
  import { get } from "svelte/store";
  import Gizmo3D from "./Gizmo3D.svelte";

  let selectedId = $derived($selectedIdStore);
  let status = $derived($obsStatus);
  let error = $derived($obsError);
  let open = $derived($openSection);

  // id court = 8 premiers caractères
  let shortId = $derived(selectedId ? selectedId.slice(0, 8) : "");

  // Widget sélectionné (pour lire mediaFit).
  let selectedWidget = $derived(
    $sceneStore.widgets.find((w) => w.id === selectedId) ?? null
  );
  let currentFit = $derived(selectedWidget?.mediaFit ?? "ajuster");

  const FIT_MODES = ["ajuster", "remplir", "etendre", "etirer", "centrer", "vignette"];

  function onFitClick(fit: string) {
    if (selectedId) setMediaFit(selectedId, fit);
  }

  async function onConnect() {
    await obsConnect(get(obsHost), get(obsPort), get(obsPassword));
  }
</script>

<aside class="sidebar">
  <!-- Section Widgets -->
  <div class="section">
    <button class="header" onclick={() => toggleSection("widgets")}>
      <span class="arrow">{open === "widgets" ? "▼" : "▶"}</span>
      <span>Widgets</span>
    </button>
    {#if open === "widgets"}
      <div class="content">
        <button class="action" onclick={createWidget}>Créer un widget</button>
        {#if !selectedId}
          <p class="hint">Cliquer un widget sur le canvas</p>
        {:else}
          <button class="action" onclick={importMedia}>Importer un média</button>
          {#if shortId}
            <label class="field">
              <span class="field-label">id</span>
              <input value={shortId} readonly spellcheck="false" />
            </label>
          {/if}
          <Gizmo3D />
          <div class="fit-group">
            <span class="field-label">Affichage</span>
            <div class="fit-buttons">
              {#each FIT_MODES as mode}
                <button
                  class="fit-btn"
                  class:active={currentFit === mode}
                  onclick={() => onFitClick(mode)}
                >{mode}</button>
              {/each}
            </div>
          </div>
        {/if}
      </div>
    {/if}
  </div>

  <!-- Section OBS -->
  <div class="section">
    <button class="header" onclick={() => toggleSection("obs")}>
      <span class="arrow">{open === "obs" ? "▼" : "▶"}</span>
      <span>OBS</span>
    </button>
    {#if open === "obs"}
      <div class="content">
        <input bind:value={$obsHost} placeholder="Hôte" spellcheck="false" />
        <input bind:value={$obsPort} placeholder="Port" spellcheck="false" />
        <input
          bind:value={$obsPassword}
          type="password"
          placeholder="Mot de passe"
          spellcheck="false"
        />
        <button
          class="action"
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
    {/if}
  </div>
</aside>

<style>
  .sidebar {
    flex-shrink: 0;
    width: 240px;
    padding: 0.5rem;
    border-right: 1px solid var(--texte);
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    overflow-y: auto;
  }
  .section {
    display: flex;
    flex-direction: column;
  }
  .header {
    background: var(--fond);
    color: var(--texte);
    border: 1px solid var(--texte);
    padding: 0.4rem 0.6rem;
    font: inherit;
    cursor: pointer;
    text-align: left;
    display: flex;
    align-items: center;
    gap: 0.4rem;
  }
  .header:hover {
    background: var(--texte);
    color: var(--fond);
  }
  .arrow {
    font-size: 0.75rem;
    width: 0.85rem;
    text-align: center;
  }
  .content {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
    padding: 0.5rem 0.6rem;
    border: 1px solid var(--texte);
    border-top: none;
  }
  .action {
    background: var(--fond);
    color: var(--texte);
    border: 1px solid var(--texte);
    padding: 0.35rem 0.6rem;
    font: inherit;
    cursor: pointer;
    text-align: left;
  }
  .action:hover {
    background: var(--texte);
    color: var(--fond);
  }
  .action:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
  .action:disabled:hover {
    background: var(--fond);
    color: var(--texte);
  }
  .hint {
    font-size: 0.8rem;
    opacity: 0.6;
    line-height: 1.3;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
  }
  .field-label {
    font-size: 0.7rem;
    opacity: 0.6;
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
    background: rgba(224, 224, 224, 0.05);
  }
  input[readonly] {
    opacity: 0.6;
    cursor: default;
  }
  .obs-status {
    font-size: 0.8rem;
    opacity: 0.7;
    word-break: break-word;
  }
  .fit-group {
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
  }
  .fit-buttons {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 0.25rem;
  }
  .fit-btn {
    background: var(--fond);
    color: var(--texte);
    border: 1px solid var(--texte);
    opacity: 0.4;
    padding: 0.3rem 0.4rem;
    font: inherit;
    font-size: 0.8rem;
    cursor: pointer;
    text-align: center;
  }
  .fit-btn:hover {
    opacity: 0.7;
  }
  .fit-btn.active {
    opacity: 1;
  }
</style>
