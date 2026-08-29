<script lang="ts">
  // Barre haute des scènes (v0.14d). Pleine largeur au-dessus du canvas.
  // 3 zones : pastilles (gauche) | nom éditable (centre) | boutons (droite).
  //
  // Pastilles : [ Nom scène ]
  //   - Clic gauche = openScene (si ≠ courante).
  //   - Clic droit = menu contextuel → « Renommer » → la pastille devient input.
  //     Enter / blur = scene_renommer. Escape = annule.
  //   - Courante = contour --texte.
  //
  // Centre : nom de la scène courante, clic → input (autre façon de renommer).
  // Boutons texte : « + Scène », « Exporter », « Importer » (pas d'icônes).
  import {
    scenesIndexStore,
    currentSceneIdStore,
    currentSceneNomStore,
    createScene,
    openScene,
    exportScene,
    importScene,
    renameScene,
    deleteScene,
  } from "../stores/scenes";

  let index = $derived($scenesIndexStore);
  let currentId = $derived($currentSceneIdStore);
  let currentNom = $derived($currentSceneNomStore);

  // ===== Menu contextuel (clic droit sur pastille) =====
  let menuSceneId = $state<string | null>(null);
  let menuX = $state(0);
  let menuY = $state(0);

  function onPastilleContext(e: MouseEvent, id: string) {
    e.preventDefault();
    e.stopPropagation();
    menuSceneId = id;
    menuX = e.clientX;
    menuY = e.clientY;
  }

  function fermerMenu() {
    menuSceneId = null;
  }

  // Fermer le menu sur clic n'importe où ailleurs
  function onWindowClick() {
    if (menuSceneId) fermerMenu();
  }

  function onWindowKeydown(e: KeyboardEvent) {
    if (e.key === "Escape" && menuSceneId) fermerMenu();
  }

  // ===== Rename inline par pastille =====
  let editingPastilleId = $state<string | null>(null);
  let pastilleNomLocal = $state("");
  let pastilleInputEl: HTMLInputElement | undefined = $state(undefined);

  function demarrerRename(id: string) {
    fermerMenu();
    editingPastilleId = id;
    pastilleNomLocal = index.find((s) => s.id === id)?.nom ?? "";
    queueMicrotask(() => pastilleInputEl?.focus());
  }

  // Confirmation suppression scène : 2 états dans le menu contextuel.
  let confirmDeleteSceneId = $state<string | null>(null);

  function demarrerSuppression(id: string) {
    confirmDeleteSceneId = id;
  }

  async function onSupprimerSceneConfirm() {
    const id = confirmDeleteSceneId;
    confirmDeleteSceneId = null;
    fermerMenu();
    if (id) await deleteScene(id);
  }

  function onSupprimerSceneCancel() {
    confirmDeleteSceneId = null;
  }

  async function onPastilleCommit(id: string) {
    if (editingPastilleId !== id) return;
    editingPastilleId = null;
    const nom = pastilleNomLocal.trim();
    const original = index.find((s) => s.id === id)?.nom ?? "";
    if (nom && nom !== original) {
      await renameScene(id, nom);
    }
  }

  function onPastilleKeydown(e: KeyboardEvent, id: string) {
    if (e.key === "Enter") {
      e.preventDefault();
      (e.target as HTMLInputElement).blur();
    } else if (e.key === "Escape") {
      editingPastilleId = null;
    }
  }

  // ===== Rename au centre (scène courante) =====
  let editingCentre = $state(false);
  let centreNomLocal = $state("");
  let centreInputEl: HTMLInputElement | undefined = $state(undefined);

  $effect(() => {
    if (!editingCentre) centreNomLocal = currentNom;
  });

  function onCentreClick() {
    centreNomLocal = currentNom;
    editingCentre = true;
    queueMicrotask(() => centreInputEl?.focus());
  }

  async function onCentreCommit() {
    if (!editingCentre) return;
    editingCentre = false;
    const nom = centreNomLocal.trim();
    if (nom && nom !== currentNom && currentId) {
      await renameScene(currentId, nom);
    } else {
      centreNomLocal = currentNom;
    }
  }

  function onCentreKeydown(e: KeyboardEvent) {
    if (e.key === "Enter") {
      e.preventDefault();
      (e.target as HTMLInputElement).blur();
    } else if (e.key === "Escape") {
      centreNomLocal = currentNom;
      editingCentre = false;
    }
  }

  function onPastille(id: string) {
    if (id !== currentId) openScene(id);
  }

  function onNouvelle() {
    const n = index.length + 1;
    createScene("Scène " + n);
  }
</script>

<svelte:window onclick={onWindowClick} onkeydown={onWindowKeydown} />

<div class="scene-bar">
  <div class="pastilles">
    {#each index as s (s.id)}
      <div class="pastille-group" class:active={s.id === currentId}>
        {#if editingPastilleId === s.id}
          <input
            class="pastille-input"
            bind:this={pastilleInputEl}
            bind:value={pastilleNomLocal}
            onblur={() => onPastilleCommit(s.id)}
            onkeydown={(e) => onPastilleKeydown(e, s.id)}
            spellcheck="false"
          />
        {:else}
          <button
            class="pastille-nom"
            onclick={() => onPastille(s.id)}
            oncontextmenu={(e) => onPastilleContext(e, s.id)}
          >{s.nom}</button>
        {/if}
      </div>
    {/each}
  </div>

  <div class="nom-centre">
    {#if editingCentre}
      <input
        class="nom-input"
        bind:this={centreInputEl}
        bind:value={centreNomLocal}
        onblur={onCentreCommit}
        onkeydown={onCentreKeydown}
        spellcheck="false"
      />
    {:else}
      <button class="nom-span" onclick={onCentreClick}>{currentNom}</button>
    {/if}
  </div>

  <div class="actions">
    <button class="action" onclick={onNouvelle}>+ Scène</button>
    <button class="action" onclick={exportScene}>Exporter</button>
    <button class="action" onclick={importScene}>Importer</button>
  </div>
</div>

{#if menuSceneId}
  <div
    class="ctx-menu"
    style="left:{menuX}px; top:{menuY}px;"
    onclick={(e) => e.stopPropagation()}
    oncontextmenu={(e) => e.preventDefault()}
    role="menu"
  >
    <button class="ctx-item" role="menuitem" onclick={() => demarrerRename(menuSceneId!)}>
      Renommer
    </button>
    {#if confirmDeleteSceneId === menuSceneId}
      <div class="ctx-confirm">
        <span class="ctx-confirm-label">Supprimer ?</span>
        <div class="ctx-confirm-buttons">
          <button class="ctx-item danger" role="menuitem" onclick={onSupprimerSceneConfirm}>Oui</button>
          <button class="ctx-item" role="menuitem" onclick={onSupprimerSceneCancel}>Non</button>
        </div>
      </div>
    {:else}
      <button class="ctx-item danger" role="menuitem" onclick={() => demarrerSuppression(menuSceneId!)}>
        Supprimer
      </button>
    {/if}
  </div>
{/if}

<style>
  .scene-bar {
    flex-shrink: 0;
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.3rem 0.6rem;
    border-bottom: 1px solid var(--texte);
    overflow: hidden;
  }
  .pastilles {
    display: flex;
    align-items: center;
    gap: 0.3rem;
    flex: 1;
    min-width: 0;
    overflow-x: auto;
  }
  .pastille-group {
    display: flex;
    align-items: stretch;
    border: 1px solid var(--texte);
    opacity: 0.5;
    flex-shrink: 0;
  }
  .pastille-group:hover {
    opacity: 0.8;
  }
  .pastille-group.active {
    opacity: 1;
    outline: 2px solid var(--texte);
    outline-offset: -2px;
  }
  .pastille-nom {
    background: var(--fond);
    color: var(--texte);
    border: none;
    padding: 0.2rem 0.6rem;
    font: inherit;
    font-size: 0.8rem;
    cursor: pointer;
    white-space: nowrap;
  }
  .pastille-input {
    background: var(--fond);
    color: var(--texte);
    border: none;
    padding: 0.2rem 0.4rem;
    font: inherit;
    font-size: 0.8rem;
    min-width: 5rem;
  }
  .pastille-input:focus {
    outline: none;
  }
  .nom-centre {
    flex-shrink: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    min-width: 8rem;
  }
  .nom-span {
    background: transparent;
    color: var(--texte);
    border: 1px solid transparent;
    padding: 0.2rem 0.6rem;
    font: inherit;
    font-size: 0.85rem;
    cursor: text;
    white-space: nowrap;
  }
  .nom-span:hover {
    border-color: var(--texte);
  }
  .nom-input {
    background: var(--fond);
    color: var(--texte);
    border: 1px solid var(--texte);
    padding: 0.2rem 0.5rem;
    font: inherit;
    font-size: 0.85rem;
    text-align: center;
    min-width: 8rem;
  }
  .nom-input:focus {
    outline: none;
  }
  .actions {
    display: flex;
    align-items: center;
    gap: 0.3rem;
    flex-shrink: 0;
  }
  .action {
    background: var(--fond);
    color: var(--texte);
    border: 1px solid var(--texte);
    padding: 0.2rem 0.5rem;
    font: inherit;
    font-size: 0.8rem;
    cursor: pointer;
    white-space: nowrap;
  }
  .action:hover {
    background: var(--texte);
    color: var(--fond);
  }
  .ctx-menu {
    position: fixed;
    z-index: 9999;
    background: var(--fond);
    border: 1px solid var(--texte);
    display: flex;
    flex-direction: column;
    min-width: 6rem;
  }
  .ctx-item {
    background: var(--fond);
    color: var(--texte);
    border: none;
    padding: 0.3rem 0.6rem;
    font: inherit;
    font-size: 0.8rem;
    cursor: pointer;
    text-align: left;
    white-space: nowrap;
  }
  .ctx-item:hover {
    background: var(--texte);
    color: var(--fond);
  }
  .ctx-item.danger:hover {
    background: #c0392b;
    color: #fff;
  }
  .ctx-confirm {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    padding: 0.2rem 0;
  }
  .ctx-confirm-label {
    font-size: 0.75rem;
    padding: 0 0.6rem;
    opacity: 0.8;
  }
  .ctx-confirm-buttons {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 0.2rem;
    padding: 0 0.2rem;
  }
</style>
