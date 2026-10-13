<script lang="ts">
  // Dropdown « Vos scènes » — bouton du header (groupe outils à droite,
  // App.svelte) ouvrant la liste des scènes sauvegardées en pastilles.
  //
  // Pastilles : [ Nom scène ]
  //   - Clic gauche = openScene (si ≠ courante).
  //   - Clic gauche + glisser = réordonner (persisté dans index.json).
  //   - Clic droit = menu contextuel → « Renommer » → la pastille devient input.
  //     Enter / blur = scene_renommer. Escape = annule.
  //   - Courante = contour --texte.
  //   - Masquée (menu contextuel) = nom remplacé par •••.
  import {
    scenesIndexStore,
    currentSceneIdStore,
    openScene,
    renameScene,
    deleteScene,
    deplacerScene,
    masquerNomScene,
  } from "../stores/scenes";
  import { dupliquerSceneModalOpen } from "../stores/ui";
  import { t } from "../stores/langue";
  import type { SceneIndex } from "../tauri";
  import CtxMenu from "./CtxMenu.svelte";

  let index = $derived($scenesIndexStore);
  let currentId = $derived($currentSceneIdStore);

  // ===== Menu contextuel (clic droit sur pastille) =====
  let menuSceneId = $state<string | null>(null);
  let menuX = $state(0);
  let menuY = $state(0);

  // ===== Dropdown "Vos scènes" (liste des pastilles) =====
  let scenesOuvert = $state(false);

  // Colonnes de la grille des pastilles : min(3, nb scènes) — une seule
  // scène = une colonne, trois et plus = 3 colonnes (« tableau » borné en
  // hauteur). Ordre ligne par ligne (gauche→droite) = ordre du document :
  // le split horizontal avant/après du drag reste cohérent.
  let colsScenes = $derived(Math.min(3, Math.max(1, index.length)));

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

  // Fermeture du menu contextuel pastille : gérée par CtxMenu (clic
  // extérieur + Escape). Ici : uniquement le dropdown "Vos scènes".
  function onWindowClick(e: MouseEvent) {
    if (scenesOuvert) {
      const target = e.target as HTMLElement;
      if (!target.closest(".vos-scenes-wrap")) {
        scenesOuvert = false;
      }
    }
  }

  function onWindowKeydown(e: KeyboardEvent) {
    if (e.key === "Escape" && scenesOuvert) scenesOuvert = false;
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

  // ===== Dupliquer (option du menu contextuel) =====
  // Ouvre la modale de nommage (DupliquerSceneModal, montée dans App.svelte)
  // avec l'id de la scène source — la création se fait au Valider.
  function onDupliquer(id: string) {
    fermerMenu();
    scenesOuvert = false;
    dupliquerSceneModalOpen.set(id);
  }
</script>

<svelte:window
  onclick={onWindowClick}
  onkeydown={onWindowKeydown}
  onmousemove={onWindowMouseMove}
  onmouseup={onWindowMouseUp}
/>

<!-- Bouton "Vos scènes" : ouvre un dropdown contenant la liste des
     scènes sauvegardées (pastilles). Le dropdown se ferme au clic
     extérieur, à Escape, ou après sélection d'une scène.
     <1000px : icône seule (title conservé). -->
<div class="vos-scenes-wrap">
  <button
    class="vos-scenes-btn"
    class:ouvert={scenesOuvert}
    onclick={toggleScenes}
    title={$t("sceneBar.vosScenes")}
    aria-label={$t("sceneBar.vosScenes")}
  >
    <svg class="ico" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M12 2 2 7l10 5 10-5-10-5z"/><path d="m2 17 10 5 10-5"/><path d="m2 12 10 5 10-5"/></svg>
    <span class="vs-label">{$t("sceneBar.vosScenes")} <span class="fleche">{scenesOuvert ? "▾" : "▸"}</span></span>
  </button>
  {#if scenesOuvert}
    <div class="pastilles-dropdown ctx-menu">
      <div class="pastilles" style="grid-template-columns: repeat({colsScenes}, minmax(8rem, auto))">
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

{#if menuSceneId}
  <CtxMenu x={menuX} y={menuY} onclose={fermerMenu}>
    <button class="ctx-item" role="menuitem" onclick={() => demarrerRename(menuSceneId!)}>
      {$t("commun.renommer")}
    </button>
    <button class="ctx-item" role="menuitem" onclick={() => onDupliquer(menuSceneId!)}>
      {$t("commun.dupliquer")}
    </button>
    {#if index.find((s) => s.id === menuSceneId)?.nomMasque}
      <button class="ctx-item" role="menuitem" onclick={() => onMasquerNom(menuSceneId!, false)}>
        {$t("sceneBar.afficherTitre")}
      </button>
    {:else}
      <button class="ctx-item" role="menuitem" onclick={() => onMasquerNom(menuSceneId!, true)}>
        {$t("sceneBar.masquerTitre")}
      </button>
    {/if}
    {#if confirmDeleteSceneId === menuSceneId}
      <div class="ctx-confirm">
        <span class="ctx-confirm-label">{$t("sceneBar.supprimerConfirm")}</span>
        <div class="ctx-confirm-buttons">
          <button class="ctx-item danger" role="menuitem" onclick={onSupprimerSceneConfirm}>{$t("commun.oui")}</button>
          <button class="ctx-item" role="menuitem" onclick={onSupprimerSceneCancel}>{$t("commun.non")}</button>
        </div>
      </div>
    {:else}
      <button class="ctx-item danger" role="menuitem" onclick={() => demarrerSuppression(menuSceneId!)}>
        {$t("commun.supprimer")}
      </button>
    {/if}
  </CtxMenu>
{/if}

<style>
  .vos-scenes-wrap {
    position: relative;
    flex-shrink: 0;
  }
  /* Bouton "Vos scènes" : ouvre le dropdown des scènes. Couleur = celle du
     cadre SVG de l'app (--cadre-couleur, poussée par App.svelte quand le
     cadre est actif) — fallback accent primaire saumon (--accent-rose)
     sinon. Seul bouton plein du header. Léger dégradé vertical + liseré
     plus sombre pour détacher la pilule du fond au lieu d'un aplat brut. */
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
    height: 2rem;
    padding: 0 0.6rem;
    font: inherit;
    font-size: 0.8rem;
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
  .vos-scenes-btn .ico {
    width: 0.95rem;
    height: 0.95rem;
    flex-shrink: 0;
  }
  .vos-scenes-btn .fleche {
    font-size: 0.7rem;
  }
  /* Dropdown : panneau .ctx-menu (app.css — verre dépoli teinté accent +
     filet dégradé, même look que les menus clic droit et le dropdown
     Langue) ancré en absolu sous le bouton à la place du fixed à la
     souris. Scroll vertical si trop de scènes, pas de scrollbar visible. */
  .pastilles-dropdown {
    position: absolute;
    top: calc(100% + 0.25rem);
    right: 0;
    min-width: 10rem;
    max-height: 60vh;
    scrollbar-width: none;
  }
  .pastilles-dropdown::-webkit-scrollbar {
    display: none;
  }
  /* Grille multi-colonnes (« tableau ») : le nombre de colonnes est posé en
     inline (repeat(colsScenes, minmax(8rem, auto))) — la liste ne s'allonge
     plus indéfiniment en hauteur. Les pistes s'adaptent au contenu ; les
     noms longs sont bornés par l'ellipse de .pastille-nom. */
  .pastilles {
    display: grid;
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
    /* Truncate : en grille multi-colonnes, un nom très long élargirait sa
       colonne — ellipse bornée pour garder le tableau compact (le nom
       complet reste lisible via l'édition / le survol). */
    max-width: 14rem;
    overflow: hidden;
    text-overflow: ellipsis;
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
  /* Menu contextuel pastille : recette .ctx-* GLOBALE dans app.css — ne
     jamais re-copier le bloc ici (même règle que .msg-pill). */
  /* <1000px : bouton « Vos scènes » en icône seule (title conservé). */
  @media (max-width: 999px) {
    .vos-scenes-btn {
      width: 2rem;
      padding: 0;
      justify-content: center;
    }
    .vos-scenes-btn .vs-label {
      display: none;
    }
  }
</style>
