<script lang="ts">
  import { createWidget, importMedia, setMediaFit, deleteWidget, selectedIdStore, sceneStore, setMediaZoomLocal, setMediaRotLocal, resetMedia, commitScene, importFond, setBgFit, setBgZoomLocal, setBgRotLocal, resetFond, clearFond, setWidgetMediaPaused, setWidgetMediaTime, setBgPaused, setBgTime } from "../stores/scene";
  import { obsConnect, obsStatus, obsError, obsHost, obsPort, obsPassword } from "../stores/obs";
  import { openSection, toggleSection } from "../stores/ui";
  import { videoRegistry } from "../stores/video";
  import { get } from "svelte/store";
  import Gizmo3D from "./Gizmo3D.svelte";
  import PlayerBar from "./PlayerBar.svelte";

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
  let currentZoom = $derived(selectedWidget?.mediaZoom ?? 1);
  let currentRot = $derived(selectedWidget?.mediaRot ?? 0);

  const FIT_MODES = ["ajuster", "remplir", "etendre", "etirer", "centrer", "vignette"];

  function onFitClick(fit: string) {
    if (selectedId) setMediaFit(selectedId, fit);
  }

  // Zoom média : oninput = maj locale (fluide), onchange (pointerup) = commit.
  function onZoomInput(e: Event) {
    if (!selectedId) return;
    setMediaZoomLocal(selectedId, parseFloat((e.target as HTMLInputElement).value));
  }
  async function onZoomChange() {
    await commitScene();
  }

  // Rotation média : même pattern.
  function onRotInput(e: Event) {
    if (!selectedId) return;
    setMediaRotLocal(selectedId, parseFloat((e.target as HTMLInputElement).value));
  }
  async function onRotChange() {
    await commitScene();
  }

  async function onResetMedia() {
    if (selectedId) await resetMedia(selectedId);
  }

  async function onConnect() {
    await obsConnect(get(obsHost), get(obsPort), get(obsPassword));
  }

  // Confirmation suppression : 2 états (idle → confirm).
  let confirmDelete = $state(false);

  function onSupprimerClick() {
    confirmDelete = true;
  }

  async function onSupprimerConfirm() {
    confirmDelete = false;
    if (selectedId) await deleteWidget(selectedId);
  }

  function onSupprimerCancel() {
    confirmDelete = false;
  }

  // ===== Fond de scène =====
  let bgMedia = $derived($sceneStore.bgMedia ?? "");
  let bgKind = $derived($sceneStore.bgKind ?? "image");
  let bgFit = $derived($sceneStore.bgFit ?? "remplir");
  let bgZoom = $derived($sceneStore.bgZoom ?? 1);
  let bgRot = $derived($sceneStore.bgRot ?? 0);
  let hasFond = $derived(bgMedia.length > 0);

  // ===== Vidéo active (barre lecteur pilote :4321) =====
  // La barre écrit mediaPaused/mediaTime dans la scène (commitScene → snapshot
  // WS → :4321 applique play/pause/seek/loop). Le dashboard reste figé.
  let selectedKind = $derived(selectedWidget?.kind ?? "image");
  let widgetMediaPaused = $derived(selectedWidget?.mediaPaused ?? true);
  let widgetMediaTime = $derived(selectedWidget?.mediaTime ?? 0);
  let widgetVideoEl = $derived(
    selectedId ? $videoRegistry.get(selectedId) : undefined
  );
  let fondVideoEl = $derived($videoRegistry.get("fond"));
  let bgPaused = $derived($sceneStore.bgPaused ?? true);
  let bgTime = $derived($sceneStore.bgTime ?? 0);

  // Callbacks barre widget.
  function onWidgetTogglePlay() {
    if (selectedId) setWidgetMediaPaused(selectedId, !widgetMediaPaused);
  }
  function onWidgetSeek(t: number) {
    if (selectedId) setWidgetMediaTime(selectedId, t);
  }

  // Callbacks barre fond.
  function onBgTogglePlay() {
    setBgPaused(!bgPaused);
  }
  function onBgSeek(t: number) {
    setBgTime(t);
  }

  function onBgFitClick(fit: string) {
    setBgFit(fit);
  }

  function onBgZoomInput(e: Event) {
    setBgZoomLocal(parseFloat((e.target as HTMLInputElement).value));
  }
  async function onBgZoomChange() {
    await commitScene();
  }

  function onBgRotInput(e: Event) {
    setBgRotLocal(parseFloat((e.target as HTMLInputElement).value));
  }
  async function onBgRotChange() {
    await commitScene();
  }

  async function onBgReset() {
    await resetFond();
  }

  async function onBgSupprimer() {
    await clearFond();
  }
</script>

<aside class="sidebar">
  <!-- Section Scène -->
  <div class="section">
    <button class="header" onclick={() => toggleSection("scene")}>
      <span class="arrow">{open === "scene" ? "▼" : "▶"}</span>
      <span>Scène</span>
    </button>
    {#if open === "scene"}
      <div class="content">
        <button class="action" onclick={importFond}>Importer un fond</button>
        {#if hasFond}
          <div class="fit-group">
            <span class="field-label">Affichage fond</span>
            <div class="fit-buttons">
              {#each FIT_MODES as mode}
                <button
                  class="fit-btn"
                  class:active={bgFit === mode}
                  onclick={() => onBgFitClick(mode)}
                >{mode}</button>
              {/each}
            </div>
          </div>
          <div class="media-ctrl">
            <div class="media-row">
              <span class="field-label">Zoom fond</span>
              <span class="media-val">{bgZoom.toFixed(2)}</span>
            </div>
            <input
              class="range"
              type="range"
              min="0.2"
              max="5"
              step="0.1"
              value={bgZoom}
              oninput={onBgZoomInput}
              onchange={onBgZoomChange}
            />
          </div>
          <div class="media-ctrl">
            <div class="media-row">
              <span class="field-label">Rotation fond</span>
              <span class="media-val">{Math.round(bgRot)}°</span>
            </div>
            <input
              class="range"
              type="range"
              min="-180"
              max="180"
              step="5"
              value={bgRot}
              oninput={onBgRotInput}
              onchange={onBgRotChange}
            />
          </div>
          <button class="action" onclick={onBgReset}>Reset fond</button>
          <button class="action" onclick={onBgSupprimer}>Supprimer le fond</button>
          {#if bgKind === "video"}
            <PlayerBar
              videoEl={fondVideoEl}
              mediaPaused={bgPaused}
              mediaTime={bgTime}
              onTogglePlay={onBgTogglePlay}
              onSeek={onBgSeek}
            />
          {/if}
        {:else}
          <p class="hint">Aucun fond. Cliquer « Importer un fond ».</p>
        {/if}
      </div>
    {/if}
  </div>

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
          <div class="media-ctrl">
            <div class="media-row">
              <span class="field-label">Zoom média</span>
              <span class="media-val">{currentZoom.toFixed(2)}</span>
            </div>
            <input
              class="range"
              type="range"
              min="0.2"
              max="5"
              step="0.1"
              value={currentZoom}
              oninput={onZoomInput}
              onchange={onZoomChange}
            />
          </div>
          <div class="media-ctrl">
            <div class="media-row">
              <span class="field-label">Rotation média</span>
              <span class="media-val">{Math.round(currentRot)}°</span>
            </div>
            <input
              class="range"
              type="range"
              min="-180"
              max="180"
              step="5"
              value={currentRot}
              oninput={onRotInput}
              onchange={onRotChange}
            />
          </div>
          <button class="action" onclick={onResetMedia}>Reset média</button>
          {#if selectedKind === "video"}
            <PlayerBar
              videoEl={widgetVideoEl}
              mediaPaused={widgetMediaPaused}
              mediaTime={widgetMediaTime}
              onTogglePlay={onWidgetTogglePlay}
              onSeek={onWidgetSeek}
            />
          {/if}
          {#if confirmDelete}
            <div class="confirm">
              <span class="confirm-label">Supprimer ce widget ?</span>
              <div class="confirm-buttons">
                <button class="action" onclick={onSupprimerConfirm}>Oui</button>
                <button class="action" onclick={onSupprimerCancel}>Non</button>
              </div>
            </div>
          {:else}
            <button class="action" onclick={onSupprimerClick}>Supprimer le widget</button>
          {/if}
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
  .media-ctrl {
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
  }
  .media-row {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
  }
  .media-val {
    font-size: 0.75rem;
    opacity: 0.8;
    font-variant-numeric: tabular-nums;
  }
  .range {
    width: 100%;
    accent-color: var(--texte);
    background: var(--fond);
    color: var(--texte);
  }
  .confirm {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
  }
  .confirm-label {
    font-size: 0.85rem;
  }
  .confirm-buttons {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 0.25rem;
  }
</style>
