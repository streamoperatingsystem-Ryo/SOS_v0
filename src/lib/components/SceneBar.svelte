<script lang="ts">
  // Barre haute des scènes (v0.14d). Pleine largeur au-dessus du canvas.
  // 4 zones : libellé "Vos scènes" (repère muet) | pastilles (gauche) |
  // nom éditable (centre) | boutons (droite).
  //
  // Pastilles : [ Nom scène ] [ œil ]
  //   - Clic gauche = openScene (si ≠ courante).
  //   - Clic gauche + glisser = réordonner (persisté dans index.json).
  //   - Clic droit = menu contextuel → « Renommer » → la pastille devient input.
  //     Enter / blur = scene_renommer. Escape = annule.
  //   - Courante = contour --texte.
  //   - Œil = masque/affiche le titre dans l'onglet (onglet orange si masqué).
  import {
    scenesIndexStore,
    currentSceneIdStore,
    currentSceneNomStore,
    openScene,
    exportScene,
    importScene,
    renameScene,
    deleteScene,
    deplacerScene,
    masquerNomScene,
  } from "../stores/scenes";
  import { presetsModalOpen } from "../stores/ui";
  import type { SceneIndex } from "../tauri";

  let index = $derived($scenesIndexStore);
  let currentId = $derived($currentSceneIdStore);
  let currentNom = $derived($currentSceneNomStore);

  // ===== Menu contextuel (clic droit sur pastille) =====
  let menuSceneId = $state<string | null>(null);
  let menuX = $state(0);
  let menuY = $state(0);

  // ===== Dropdown "Vos scènes" (liste des pastilles) =====
  let scenesOuvert = $state(false);

  function toggleScenes() {
    scenesOuvert = !scenesOuvert;
  }

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
  function onWindowClick(e: MouseEvent) {
    if (menuSceneId) fermerMenu();
    // Fermer le dropdown "Vos scènes" si clic en dehors
    if (scenesOuvert) {
      const target = e.target as HTMLElement;
      if (!target.closest(".vos-scenes-wrap")) {
        scenesOuvert = false;
      }
    }
  }

  function onWindowKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      if (menuSceneId) fermerMenu();
      if (scenesOuvert) scenesOuvert = false;
    }
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

  function onPastilleKeydown(e: KeyboardEvent, _id: string) {
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
    // Après un drag, suppressClick empêche le clic de suivre d'ouvrir la scène
    if (suppressClick) {
      suppressClick = false;
      return;
    }
    if (id !== currentId) openScene(id);
    scenesOuvert = false;
  }

  // ===== Drag & drop : réordonner les scènes (clic gauche + glisser) =====
  // Pointer-based (mousedown/mousemove/mouseup) — plus fiable que HTML5 DnD
  // dans un webview Tauri (dragDropEnabled=true intercepte dragover/drop).
  // - mousedown sur une pastille → enregistre position (drag potentiel)
  // - mousemove global au-delà d'un seuil (5px) → drag réel commence
  // - mouseup global → si drag : réordonner + persister ; sinon : clic normal
  //   (le onclick du bouton ouvre la scène)
  let draggedId = $state<string | null>(null);
  let dragEnCours = $state(false);
  let dragPendingId: string | null = null;
  let dragStartX = 0;
  let dragStartY = 0;
  let dragHoverId = $state<string | null>(null);
  let dragHoverApres = $state(false); // true = insérer après la pastille survolée
  let suppressClick = false; // empêche le onclick du bouton après un drag
  let ordreAvantDrag: SceneIndex[] = [];

  const DRAG_SEUIL = 5; // px

  function onPastilleMouseDown(e: MouseEvent, s: SceneIndex) {
    // Bouton gauche seulement ; pas pendant l'édition inline
    if (e.button !== 0 || editingPastilleId === s.id) return;
    dragPendingId = s.id;
    dragStartX = e.clientX;
    dragStartY = e.clientY;
  }

  function onWindowMouseMove(e: MouseEvent) {
    // Pas de drag en cours → vérifier seuil
    if (!dragEnCours && dragPendingId) {
      const dx = e.clientX - dragStartX;
      const dy = e.clientY - dragStartY;
      if (Math.abs(dx) < DRAG_SEUIL && Math.abs(dy) < DRAG_SEUIL) return;
      // Seuil franchi → démarrer le drag
      dragEnCours = true;
      draggedId = dragPendingId;
      ordreAvantDrag = [...index];
    }
    if (!dragEnCours || !draggedId) return;
    // Trouver la pastille sous le curseur via elementFromPoint
    const el = document.elementFromPoint(e.clientX, e.clientY);
    if (!el) return;
    const groupEl = (el as HTMLElement).closest(".pastille-group");
    if (!groupEl) {
      dragHoverId = null;
      return;
    }
    const hoverId = groupEl.getAttribute("data-id");
    if (!hoverId || hoverId === draggedId) {
      dragHoverId = null;
      return;
    }
    const rect = groupEl.getBoundingClientRect();
    dragHoverId = hoverId;
    dragHoverApres = e.clientX > rect.left + rect.width / 2;
  }

  async function onWindowMouseUp(_e: MouseEvent) {
    if (dragEnCours && draggedId) {
      // Drag a eu lieu → réordonner selon la dernière cible survolée
      if (dragHoverId) {
        const entry = ordreAvantDrag.find((x) => x.id === draggedId);
        if (entry) {
          const idx = ordreAvantDrag.filter((x) => x.id !== draggedId);
          let to = idx.findIndex((x) => x.id === dragHoverId);
          if (to >= 0) {
            if (dragHoverApres) to += 1;
            idx.splice(to, 0, entry);
            scenesIndexStore.set(idx);
            const position = idx.findIndex((x) => x.id === draggedId);
            if (position >= 0) await deplacerScene(draggedId, position);
          }
        }
      }
      // Empêcher le clic qui suit le mouseup d'ouvrir la scène
      suppressClick = true;
    }
    dragEnCours = false;
    dragPendingId = null;
    draggedId = null;
    dragHoverId = null;
    ordreAvantDrag = [];
  }

  // ===== Masquer/afficher le titre (option du menu contextuel) =====
  function onMasquerNom(id: string, masque: boolean) {
    fermerMenu();
    void masquerNomScene(id, masque);
  }

  function onNouvelle() {
    // Ouvre la galerie de presets (page vierge + modèles embarqués) — la
    // création elle-même se fait dans PresetsModal (mode "creation").
    presetsModalOpen.set("creation");
  }
</script>

<svelte:window
  onclick={onWindowClick}
  onkeydown={onWindowKeydown}
  onmousemove={onWindowMouseMove}
  onmouseup={onWindowMouseUp}
/>

<div class="scene-bar">
  <!-- Bouton "Vos scènes" : ouvre un dropdown contenant la liste des
       scènes sauvegardées (pastilles). Le dropdown se ferme au clic
       extérieur, à Escape, ou après sélection d'une scène. -->
  <div class="vos-scenes-wrap">
    <button class="vos-scenes-btn" class:ouvert={scenesOuvert} onclick={toggleScenes}>
      <svg class="ico" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M12 2 2 7l10 5 10-5-10-5z"/><path d="m2 17 10 5 10-5"/><path d="m2 12 10 5 10-5"/></svg>
      Vos scènes <span class="fleche">{scenesOuvert ? "▾" : "▸"}</span>
    </button>
    {#if scenesOuvert}
      <div class="pastilles-dropdown">
        <div class="pastilles">
          {#each index as s (s.id)}
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div
              class="pastille-group"
              class:active={s.id === currentId}
              class:masquee={s.nomMasque === true}
              class:drag-source={draggedId === s.id}
              class:drop-before={dragEnCours && dragHoverId === s.id && !dragHoverApres}
              class:drop-after={dragEnCours && dragHoverId === s.id && dragHoverApres}
              data-id={s.id}
              onmousedown={(e) => onPastilleMouseDown(e, s)}
            >
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
                  class:masque={s.nomMasque === true}
                  onclick={() => onPastille(s.id)}
                  oncontextmenu={(e) => onPastilleContext(e, s.id)}
                  title={s.nomMasque ? s.nom : ""}
                >
                  {#if s.nomMasque}
                    <span class="nom-cache-texte" aria-hidden="true">{s.nom}</span>
                    <span class="nom-cache-dots">•••</span>
                  {:else}
                    {s.nom}
                  {/if}
                </button>
              {/if}
            </div>
          {/each}
        </div>
      </div>
    {/if}
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
    <button class="action" onclick={exportScene}>
      <svg class="ico" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="17 8 12 3 7 8"/><line x1="12" y1="3" x2="12" y2="15"/></svg>
      Exporter
    </button>
    <button class="action" onclick={importScene}>
      <svg class="ico" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>
      Importer
    </button>
  </div>
</div>

{#if menuSceneId}
  <div
    class="ctx-menu"
    style="left:{menuX}px; top:{menuY}px;"
    onclick={(e) => e.stopPropagation()}
    oncontextmenu={(e) => e.preventDefault()}
    onkeydown={(e) => e.stopPropagation()}
    role="menu"
    tabindex="-1"
  >
    <button class="ctx-item" role="menuitem" onclick={() => demarrerRename(menuSceneId!)}>
      Renommer
    </button>
    {#if index.find((s) => s.id === menuSceneId)?.nomMasque}
      <button class="ctx-item" role="menuitem" onclick={() => onMasquerNom(menuSceneId!, false)}>
        Afficher le titre
      </button>
    {:else}
      <button class="ctx-item" role="menuitem" onclick={() => onMasquerNom(menuSceneId!, true)}>
        Masquer le titre
      </button>
    {/if}
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
    border-bottom: 1px solid var(--bordure-ui);
    background: transparent; /* laisse transparaître le fond d'ambiance */
    position: relative;
    z-index: 200;
    /* SceneBar = zone identité néon (signature orange/magenta) : l'orange
       vit dans les bordures/glows/états actifs et le texte — le fill des
       boutons reste la glace neutre commune (IDEA02). */
  }
  /* Bouton "Vos scènes" : ouvre le dropdown des scènes. Couleur = celle du
     cadre SVG de l'app (--cadre-couleur, poussée par App.svelte quand le
     cadre est actif) — fallback accent primaire saumon (--accent-rose)
     sinon. Seul bouton plein de la barre. Léger dégradé vertical + liseré
     plus sombre pour détacher la pilule du fond au lieu d'un aplat brut. */
  .vos-scenes-wrap {
    position: relative;
    flex-shrink: 0;
  }
  .vos-scenes-btn {
    --vos-scenes: var(--cadre-couleur, var(--accent-rose));
    background:
      linear-gradient(
        180deg,
        color-mix(in srgb, var(--vos-scenes) 88%, white) 0%,
        var(--vos-scenes) 100%
      );
    box-shadow: var(--btn-inset);
    color: var(--texte);
    /* Texte adaptatif : la couleur choisie peut être claire (blanc…) →
       bascule sombre quand la luminosité oklch dépasse ~0.72 (relative
       color syntax, Chromium ≥119 ; invalide → fallback var(--texte)). */
    color: oklch(from var(--vos-scenes) clamp(0, (0.72 - l) * 100, 0.95) 0 0);
    border: 1px solid color-mix(in srgb, var(--vos-scenes) 55%, var(--fond));
    border-radius: var(--rayon-petit);
    padding: 0.2rem 0.6rem;
    font: inherit;
    font-size: 0.78rem;
    cursor: pointer;
    white-space: nowrap;
    display: flex;
    align-items: center;
    gap: 0.35rem;
    transition: box-shadow 0.15s ease, background 0.15s ease, border-color 0.15s ease;
  }
  .vos-scenes-btn:hover,
  .vos-scenes-btn.ouvert {
    background:
      linear-gradient(
        180deg,
        color-mix(in srgb, var(--vos-scenes) 72%, white) 0%,
        color-mix(in srgb, var(--vos-scenes) 90%, white) 100%
      );
    border-color: color-mix(in srgb, var(--vos-scenes) 40%, white);
    box-shadow:
      var(--btn-inset-hover),
      0 0 10px color-mix(in srgb, var(--vos-scenes) 45%, transparent);
  }
  .vos-scenes-btn .fleche {
    font-size: 0.7rem;
  }
  /* Dropdown : panneau sous le bouton, fond panneau + bordure active +
     glow violet. Scroll vertical si trop de scènes, pas de scrollbar
     visible (scrollbar-width: none + ::-webkit-scrollbar hidden). */
  .pastilles-dropdown {
    position: absolute;
    top: calc(100% + 0.25rem);
    left: 0;
    z-index: 1000;
    background: var(--fond-panneau);
    border: 1px solid var(--bordure-ui-active);
    border-radius: var(--rayon-petit);
    box-shadow: 0 0 20px color-mix(in srgb, var(--accent-ui) 20%, transparent);
    padding: 0.3rem;
    min-width: 10rem;
    max-height: 60vh;
    overflow-y: auto;
    scrollbar-width: none;
  }
  .pastilles-dropdown::-webkit-scrollbar {
    display: none;
  }
  .pastilles {
    display: flex;
    flex-direction: column;
    align-items: stretch;
    gap: 0.25rem;
  }
  .pastille-group {
    display: flex;
    align-items: stretch;
    border: 1px solid var(--bordure-ui-active);
    border-radius: var(--rayon-petit);
    background: var(--btn-surface);
    box-shadow: var(--btn-inset);
    opacity: 0.5;
    flex-shrink: 0;
    width: 100%;
    transition: opacity 0.15s ease, border-color 0.15s ease, box-shadow 0.15s ease, background 0.15s ease;
  }
  .pastille-group:hover {
    background: var(--btn-surface-hover);
    box-shadow: var(--btn-inset-hover);
    opacity: 0.8;
    border-color: var(--bordure-ui-active);
  }
  .pastille-group.active {
    opacity: 1;
    border: 1px solid transparent;
    background:
      var(--btn-surface-hover) padding-box,
      var(--gradient-cadre) border-box border-area;
    box-shadow:
      var(--btn-inset),
      var(--glow-orange),
      0 0 6px color-mix(in srgb, var(--accent-magenta) 30%, transparent);
  }
  .pastille-nom {
    background: transparent;
    color: var(--texte);
    border: none;
    border-radius: var(--rayon-petit);
    padding: 0.25rem 0.6rem;
    font: inherit;
    font-size: 0.8rem;
    cursor: pointer;
    white-space: nowrap;
    width: 100%;
    text-align: left;
  }
  .pastille-input {
    background: var(--fond-controle);
    color: var(--texte);
    border: none;
    border-radius: var(--rayon-petit);
    padding: 0.25rem 0.4rem;
    font: inherit;
    font-size: 0.8rem;
    width: 100%;
  }
  .pastille-input:focus {
    outline: none;
  }
  /* Titre masqué : le bouton garde sa taille (nom invisible conservé pour
     la largeur), seul le bord + les pointillés deviennent orange vif.
     Fond identique à l'état normal — pas de couleur pleine. */
  .pastille-nom.masque {
    position: relative;
    background: var(--btn-surface);
    color: transparent;
    border: 1px solid transparent;
    border-radius: var(--rayon-petit);
  }
  .nom-cache-texte {
    /* Nom réel, invisible — préserve la largeur exacte du bouton. */
    color: transparent;
  }
  .nom-cache-dots {
    /* Pointillés orange centrés sur le bouton, par-dessus le nom invisible. */
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--surbrillance-orange);
    pointer-events: none;
  }
  .pastille-group.masquee {
    border: 1px solid transparent;
    border-radius: var(--rayon-petit);
    background:
      var(--btn-surface) padding-box,
      var(--gradient-cadre) border-box border-area;
    box-shadow:
      var(--btn-inset),
      0 0 8px color-mix(in srgb, var(--accent-orange) 40%, transparent),
      0 0 4px color-mix(in srgb, var(--accent-magenta) 30%, transparent);
    /* Forcer l'opacité pour que l'orange reste visible même à l'état
       non-actif (opacity 0.5 par défaut atténuerait la bordure orange). */
    opacity: 0.95;
  }
  /* Source de drag : semi-transparent pendant le glisser. */
  .pastille-group.drag-source {
    opacity: 0.35;
  }
  /* Indicateur de drop : barre orange avant/après la pastille survolée. */
  .pastille-group.drop-before {
    box-shadow: -2px 0 0 0 var(--accent-orange);
  }
  .pastille-group.drop-after {
    box-shadow: 2px 0 0 0 var(--accent-orange);
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
    border-color: var(--accent-ui);
  }
  .nom-input {
    background: var(--fond-controle);
    color: var(--texte);
    border: 1px solid var(--bordure-ui-active);
    padding: 0.2rem 0.5rem;
    font: inherit;
    font-size: 0.85rem;
    text-align: center;
    min-width: 8rem;
  }
  .nom-input:focus {
    outline: none;
    border-color: var(--accent-ui);
  }
  .actions {
    display: flex;
    align-items: center;
    gap: 0.3rem;
    flex-shrink: 0;
  }
  .action {
    background: var(--btn-surface);
    box-shadow: var(--btn-inset);
    color: var(--texte);
    border: 1px solid var(--bordure-ui-active);
    border-radius: var(--rayon-petit);
    padding: 0.2rem 0.5rem;
    font: inherit;
    font-size: 0.8rem;
    cursor: pointer;
    white-space: nowrap;
    display: flex;
    align-items: center;
    gap: 0.3rem;
    transition: border-color 0.15s ease, box-shadow 0.15s ease, background 0.15s ease;
  }
  .action:hover {
    border: 1px solid transparent;
    background:
      var(--btn-surface-hover) padding-box,
      var(--gradient-cadre) border-box border-area;
    box-shadow:
      var(--btn-inset-hover),
      var(--glow-orange),
      0 0 6px color-mix(in srgb, var(--accent-magenta) 30%, transparent);
  }
  .action:active {
    background:
      var(--btn-surface-active) padding-box,
      var(--gradient-cadre) border-box border-area;
    box-shadow: var(--btn-inset-active);
  }
  .ctx-menu {
    position: fixed;
    z-index: 9999;
    background: var(--fond-panneau);
    border: 1px solid var(--bordure-ui-active);
    box-shadow: 0 0 20px color-mix(in srgb, var(--accent-ui) 20%, transparent);
    display: flex;
    flex-direction: column;
    min-width: 6rem;
  }
  .ctx-item {
    background: var(--fond-panneau);
    color: var(--texte);
    border: none;
    border-left: 2px solid transparent;
    padding: 0.3rem 0.6rem;
    font: inherit;
    font-size: 0.8rem;
    cursor: pointer;
    text-align: left;
    white-space: nowrap;
  }
  .ctx-item:hover {
    border-left-color: var(--accent-ui);
    background: var(--btn-surface-hover);
  }
  .ctx-item.danger:hover {
    background: var(--message-error-color);
    color: #fff;
    border-left-color: var(--message-error-color);
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
