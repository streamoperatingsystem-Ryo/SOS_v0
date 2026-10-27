<script lang="ts">
  // Dropdown « Vos scènes » — bouton du header (groupe outils à droite,
  // App.svelte) ouvrant la liste des scènes sauvegardées en tableau centré
  // dans le viewport (voile fixe : clic = ferme).
  //
  // Pastilles : [ Nom scène ]
  //   - Clic gauche = openScene (si ≠ courante).
  //   - Clic gauche + glisser = réordonner (persisté dans index.json).
  //   - Clic droit = menu contextuel → « Renommer » → la pastille devient input.
  //     Enter / blur = scene_renommer. Escape = annule.
  //   - Courante = contour --texte.
  //   - Masquée (menu contextuel) = titre affiché en orange ; son ouverture
  //     demande une confirmation (ConfirmOuvrirSceneModal).
  import {
    scenesIndexStore,
    currentSceneIdStore,
    scenesResumeStore,
    loadScenesResume,
    openScene,
    renameScene,
    deleteScene,
    deplacerScene,
    masquerNomScene,
  } from "../stores/scenes";
  import { confirmOuvrirSceneModalOpen, dupliquerSceneModalOpen } from "../stores/ui";
  import { t } from "../stores/langue";
  import type { SceneIndex } from "../tauri";
  import CtxMenu from "./CtxMenu.svelte";

  let index = $derived($scenesIndexStore);
  let currentId = $derived($currentSceneIdStore);
  // Résumés par scène (nb widgets, types, mini-map) — rechargés à chaque
  // ouverture du tableau (vérité disque, petits fichiers JSON).
  let resumes = $derived($scenesResumeStore);
  // Regroupement distinct : les scènes masquées (titre orange) s'affichent
  // APRÈS les visibles, en bloc séparé par un filet labelisé. Ordre relatif
  // conservé dans chaque groupe — le drag & drop continue de travailler
  // sur l'ordre RÉEL de l'index (ordreAvantDrag) ; une masquée déplacée
  // parmi les visibles revient visuellement dans son groupe.
  let indexTri = $derived([
    ...index.filter((s) => s.nomMasque !== true),
    ...index.filter((s) => s.nomMasque === true),
  ]);
  let nbVisibles = $derived(index.length - index.filter((s) => s.nomMasque === true).length);

  /// Libellé traduit d'un type de widget (zone carteEdition.type.*) —
  /// repli sur l'id brut si la clé manque (type inconnu/legacy).
  function labelType(ty: string): string {
    const v = $t("carteEdition.type." + ty);
    return v === "carteEdition.type." + ty ? ty : v;
  }

  /// Libellé traduit d'un module HUD (zone modules.nom.*) — même repli.
  function labelModule(m: string): string {
    const v = $t("modules.nom." + m);
    return v === "modules.nom." + m ? m : v;
  }

  // ===== Menu contextuel (clic droit sur pastille) =====
  let menuSceneId = $state<string | null>(null);
  let menuX = $state(0);
  let menuY = $state(0);

  // ===== Dropdown "Vos scènes" (tableau des pastilles) =====
  let scenesOuvert = $state(false);

  function toggleScenes() {
    scenesOuvert = !scenesOuvert;
    // Recharge les résumés (nb widgets, types, mini-map) à chaque ouverture.
    if (scenesOuvert) void loadScenesResume();
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
    // Pastille masquée (titre orange) : confirmation avant ouverture
    // (ConfirmOuvrirSceneModal). Scène courante : rien à ouvrir, on ferme
    // juste le dropdown comme avant.
    const cible = index.find((s) => s.id === id);
    if (id !== currentId && cible?.nomMasque === true) {
      confirmOuvrirSceneModalOpen.set(id);
      scenesOuvert = false;
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

<!-- Bouton "Vos scènes" : ouvre le tableau des scènes sauvegardées
     (pastilles), centré dans le viewport. Fermeture : clic sur le voile /
     extérieur, Escape, ou après sélection d'une scène.
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
    <!-- Voile plein écran sous le tableau : clic = fermer (équivalent du
         clic extérieur — il est DANS .vos-scenes-wrap, d'où le onclick
         dédié). role="presentation" : pattern Modal.svelte. -->
    <div class="pastilles-backdrop" role="presentation" onclick={() => (scenesOuvert = false)}></div>
    <div class="pastilles-dropdown ctx-menu">
      <div class="pastilles">
        {#each indexTri as s, i (s.id)}
          <!-- Bandeaux de groupe pleine largeur (même recette orange) :
               « Scènes visibles » en tête, « Scènes masquées » avant la
               1re masquée. Chacun n'apparaît que si son groupe existe. -->
          {#if i === 0 && nbVisibles > 0}
            <div class="pastilles-sep"><span>{$t("sceneBar.groupeVisibles")}</span></div>
          {/if}
          {#if i === nbVisibles}
            <div class="pastilles-sep"><span>{$t("sceneBar.groupeMasquees")}</span></div>
          {/if}
          {@const resume = resumes[s.id]}
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
                class="pastille-cell"
                title={s.nom}
                onclick={() => onPastille(s.id)}
                oncontextmenu={(e) => onPastilleContext(e, s.id)}
              >
                {#if resume?.ok && resume.canvasW > 0 && resume.canvasH > 0}
                  {@const wTriees = [...resume.widgets].sort((a, b) => a.z - b.z)}
                  <!-- Mini-map : cadre au ratio du canvas OBS de la scène,
                       chaque widget = un rectangle coloré par type. -->
                  <span
                    class="mini-map"
                    class:avec-fond={resume.aFond}
                    style="aspect-ratio: {resume.canvasW} / {resume.canvasH}"
                  >
                    {#each wTriees as w}
                      <span
                        class="mm-widget"
                        class:trou={w.trou}
                        data-type={w.type}
                        title={labelType(w.type)}
                        style="left:{(w.x / resume.canvasW) * 100}%; top:{(w.y / resume.canvasH) * 100}%; width:{(w.largeur / resume.canvasW) * 100}%; height:{(w.hauteur / resume.canvasH) * 100}%"
                      ></span>
                    {/each}
                  </span>
                {/if}
                <span class="pastille-nom" class:masque={s.nomMasque === true}>{s.nom}</span>
                {#if resume}
                  <span class="pastille-info">
                    {#if resume.ok}
                      <span class="nb-widgets">{$t("sceneBar.nbWidgets", { n: resume.nbWidgets })}</span>
                      {#each resume.types as ty}
                        <span class="type-dot" data-type={ty} title={labelType(ty)}></span>
                      {/each}
                      {#each resume.modules as m}
                        <span class="mod-chip" title={labelModule(m)}>{labelModule(m)}</span>
                      {/each}
                      {#if s.id === currentId}
                        <span class="badge-antenne">{$t("sceneBar.antenne")}</span>
                      {/if}
                    {:else}
                      <span class="nb-widgets illisible">{$t("sceneBar.illisible")}</span>
                    {/if}
                  </span>
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
        {$t("sceneBar.afficherScene")}
      </button>
    {:else}
      <button class="ctx-item" role="menuitem" onclick={() => onMasquerNom(menuSceneId!, true)}>
        {$t("sceneBar.masquerScene")}
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
  /* Voile plein écran sous le tableau : assombrit le dashboard, clic =
     ferme. z 9990 : au-dessus des overlays du canvas (~9600) mais SOUS le
     panneau ctx-menu (9999) et la modale de confirmation (10000). */
  .pastilles-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.55);
    z-index: 9990;
  }
  /* Tableau « Vos scènes » : panneau .ctx-menu (verre dépoli teinté accent)
     recentré dans le VIEWPORT (fixed hérité de .ctx-menu — z 9999, sous la
     modale de confirmation à 10000). Largeur adaptative bornée : le tableau
     reste centré quel que soit le format/résolution ; scroll vertical si
     trop de scènes, sans scrollbar visible. */
  .pastilles-dropdown {
    top: 50%;
    left: 50%;
    width: min(64rem, 92vw);
    max-height: 80vh;
    scrollbar-width: none;
    animation: scenes-pop 0.12s ease-out;
    transform: translate(-50%, -50%);
    transform-origin: center;
  }
  .pastilles-dropdown::-webkit-scrollbar {
    display: none;
  }
  /* L'animation ctx-pop de .ctx-menu réécrit transform pendant 0,12 s et
     casserait le translate(-50%,-50%) → variante dédiée qui l'inclut. */
  @keyframes scenes-pop {
    from {
      opacity: 0;
      transform: translate(-50%, calc(-50% - 3px)) scale(0.98);
    }
    to {
      opacity: 1;
      transform: translate(-50%, -50%) scale(1);
    }
  }
  /* Grille = vrai tableau : colonnes fluides ÉGALES (auto-fit → les pistes
     vides se replient et les cellules remplissent toute la largeur, adapté
     à toutes les résolutions). Traits bien marqués : bordure visible sur
     chaque cellule + cadre du tableau → séparateurs uniformes à 2 px
     (1 px cellule + 1 px voisine, 1 px cellule + 1 px cadre). */
  .pastilles {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(9rem, 1fr));
    border: 1px solid var(--bordure-ui-active);
    border-radius: var(--rayon-petit);
    overflow: hidden;
    background: var(--btn-surface);
  }
  /* Bandeau de groupe pleine largeur : libellé flanqué de filets dégradé
     orange — MÊME recette pour « Scènes visibles » (en-tête) et
     « Scènes masquées » (avant le bloc masqué), rend le regroupement
     immédiatement lisible dans le tableau. */
  .pastilles-sep {
    grid-column: 1 / -1;
    display: flex;
    align-items: center;
    gap: 0.6rem;
    padding: 0.3rem 0.6rem;
    font-size: 0.62rem;
    font-weight: 700;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: var(--surbrillance-orange);
    border-top: 1px solid color-mix(in srgb, var(--accent-orange) 45%, transparent);
    border-bottom: 1px solid color-mix(in srgb, var(--accent-orange) 45%, transparent);
    background: color-mix(in srgb, var(--accent-orange) 6%, transparent);
  }
  .pastilles-sep::before,
  .pastilles-sep::after {
    content: "";
    flex: 1;
    height: 1px;
    background: color-mix(in srgb, var(--accent-orange) 40%, transparent);
  }
  .pastille-group {
    display: flex;
    align-items: stretch;
    border: 1px solid var(--bordure-ui);
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
  /* Cellule enrichie : bouton pleine cellule empilant mini-map + nom + ligne
     d'infos (min-width:0 sinon le nom long empêche la troncature). */
  .pastille-cell {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
    align-items: stretch;
    background: transparent;
    color: var(--texte);
    border: none;
    padding: 0.4rem;
    font: inherit;
    cursor: pointer;
    width: 100%;
    min-width: 0;
  }
  /* Mini-map procédurale : cadre au ratio du canvas OBS de la scène
     (aspect-ratio inline), chaque widget = rectangle positionné en %.
     Teintée accent si la scène a un fond (média/carrousel/source OBS). */
  .mini-map {
    position: relative;
    display: block;
    width: 100%;
    border-radius: 3px;
    overflow: hidden;
    background: rgba(0, 0, 0, 0.45);
    border: 1px solid var(--bordure-ui);
  }
  .mini-map.avec-fond {
    background: color-mix(in srgb, var(--accent-ui) 16%, rgba(0, 0, 0, 0.5));
    border-color: color-mix(in srgb, var(--accent-ui) 45%, var(--bordure-ui));
  }
  .mm-widget {
    position: absolute;
    background: var(--tc, var(--gris));
    opacity: 0.8;
    border-radius: 1px;
  }
  /* Trous (fenêtres de capture OBS) : contour pointillé sans remplissage —
     la couleur reste celle du type du widget porteur. */
  .mm-widget.trou {
    background: transparent;
    border: 1px dashed var(--tc, var(--gris));
    box-sizing: border-box;
    opacity: 0.95;
  }
  /* Couleurs par type de widget — partagées mini-map (.mm-widget) et points
     de légende (.type-dot) via la custom prop --tc. */
  [data-type="media"] { --tc: #a78bfa; }
  [data-type="chat"] { --tc: #38bdf8; }
  [data-type="camera"] { --tc: #f472b6; }
  [data-type="input-viewer"] { --tc: #facc15; }
  [data-type="speedrun"] { --tc: #4ade80; }
  [data-type="goal"] { --tc: #fb923c; }
  [data-type="musique"] { --tc: #2dd4bf; }
  .pastille-nom {
    color: var(--texte);
    font-size: 0.8rem;
    white-space: nowrap;
    /* Truncate : en grille multi-colonnes, un nom très long élargirait sa
       colonne — ellipse bornée pour garder le tableau compact (le nom
       complet reste lisible via l'édition / le survol). */
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    text-align: center;
  }
  /* Ligne d'infos sous le nom : nb widgets + points par type + modules HUD
     configurés + badge « À l'antenne » sur la scène courante. */
  .pastille-info {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: center;
    gap: 0.25rem;
    font-size: 0.62rem;
    line-height: 1.2;
    color: var(--gris);
  }
  .nb-widgets {
    font-weight: 600;
    letter-spacing: 0.02em;
  }
  .nb-widgets.illisible {
    color: var(--message-error-color);
    font-style: italic;
  }
  .type-dot {
    width: 0.5rem;
    height: 0.5rem;
    border-radius: 50%;
    background: var(--tc, var(--gris));
    flex-shrink: 0;
  }
  .mod-chip {
    border: 1px solid var(--bordure-ui);
    border-radius: var(--rayon-petit);
    padding: 0 0.25rem;
    font-size: 0.58rem;
    color: var(--gris);
    white-space: nowrap;
  }
  /* Badge « À l'antenne » : pastille rouge style LIVE sur la scène courante. */
  .badge-antenne {
    background: var(--message-error-color);
    color: #fff;
    border-radius: var(--rayon-petit);
    padding: 0.05rem 0.35rem;
    font-size: 0.58rem;
    font-weight: 700;
    letter-spacing: 0.04em;
    text-transform: uppercase;
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
  /* Scène masquée : le titre reste affiché, en orange vif — même rappel
     que la bordure/glow du groupe (.pastille-group.masquee). */
  .pastille-nom.masque {
    color: var(--surbrillance-orange);
  }
  .pastille-group.masquee {
    border: 1px solid transparent;
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
