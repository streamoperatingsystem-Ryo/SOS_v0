<script lang="ts">
  // Bandeau drop médias : zone « Glisser images ou vidéos ici » + liste des
  // fichiers droppés (noms seuls) + bouton rouge « Transformer en widget ! »
  // à droite, visible uniquement quand le widget sélectionné est un drop
  // (sansCadre === true). Pleine largeur de .canvas-col,
  // rendu entre SceneBar et Canvas (App.svelte).
  //
  // Le drop passe par l'event Tauri (dragDropEnabled=true intercepte le HTML5
  // drop) — aucun handler dragover/drop DOM. La position DragDropEvent est en
  // pixels PHYSIQUES ; getBoundingClientRect est en CSS px → /dndScale.
  // Hit-test = rect du bandeau entier (label + liste) : drop sur le canvas ou
  // la sidebar = aucun import.
  import { bibliothequeMedias, ajouterALaBibliotheque, transformerEnWidget, sceneStore, selectedIdStore } from "../stores/scene";
  import { onMount, onDestroy } from "svelte";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { getCurrentWindow } from "@tauri-apps/api/window";

  let biblio = $derived($bibliothequeMedias);
  // Widget sélectionné : le bouton « Transformer en widget ! » n'apparaît
  // que pour un widget droppé (sansCadre === true) avec un média.
  let selectedWidget = $derived($sceneStore.widgets.find((w) => w.id === $selectedIdStore));
  let dropZoneEl = $state<HTMLElement | null>(null);
  let dropHover = $state(false);
  let dndScale = 1;
  let unlistenDrop: (() => void) | null = null;

  function inDropZone(pos: { x: number; y: number }): boolean {
    if (!dropZoneEl) return false;
    const r = dropZoneEl.getBoundingClientRect();
    const x = pos.x / dndScale;
    const y = pos.y / dndScale;
    return x >= r.left && x <= r.right && y >= r.top && y <= r.bottom;
  }

  onMount(async () => {
    try { dndScale = await getCurrentWindow().scaleFactor(); } catch {}
    try {
      unlistenDrop = await getCurrentWebview().onDragDropEvent((event) => {
        const p = event.payload;
        if (p.type === "enter" || p.type === "over") {
          dropHover = inDropZone(p.position);
        } else if (p.type === "drop") {
          dropHover = false;
          if (inDropZone(p.position)) void ajouterALaBibliotheque(p.paths);
        } else {
          dropHover = false;
        }
      });
    } catch {}
  });

  onDestroy(() => {
    unlistenDrop?.();
  });
</script>

<div class="drop-bar" class:hover={dropHover} bind:this={dropZoneEl}>
  <span class="drop-label">Glisser images ou vidéos ici</span>
  <div class="droite">
    {#if biblio.length > 0}
      <div class="biblio">
        {#each biblio as m (m.rel)}
          <div class="biblio-row">
            <span class="biblio-kind">{m.kind === "video" ? "🎬" : "🖼"}</span>
            <span class="biblio-nom" title={m.nom}>{m.nom}</span>
          </div>
        {/each}
      </div>
    {/if}
    {#if selectedWidget?.sansCadre === true && selectedWidget.media}
      <button
        class="action danger"
        onclick={() => transformerEnWidget(selectedWidget.media!)}
      >Transformer en widget !</button>
    {/if}
  </div>
</div>

<style>
  /* Bandeau horizontal pleine largeur, bordure pointillée. Hauteur bornée :
     la liste wrappe puis scrolle verticalement si beaucoup de fichiers. */
  .drop-bar {
    flex-shrink: 0;
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 0.25rem 0.6rem;
    padding: 0.3rem 0.6rem;
    border: 1px dashed var(--bordure);
    background: var(--fond-panneau);
    font-size: 0.82rem;
    color: var(--texte);
    opacity: 0.8;
    max-height: 5.5rem;
    overflow-y: auto;
    transition: border-color 0.15s ease, box-shadow 0.15s ease, opacity 0.15s ease;
  }
  /* .hover = curseur Tauri DANS la zone (pas un hover CSS — la position
     physique de l'event drop est comparée au rect de l'élément). */
  .drop-bar.hover {
    border-color: var(--dash-accent);
    box-shadow: var(--dash-glow);
    opacity: 1;
  }
  .drop-label {
    flex-shrink: 0;
    white-space: nowrap;
  }
  /* Groupe de droite (noms + bouton Transformer) : poussé au bord droit du
     bandeau — le bouton reste à droite même quand la liste est vide. */
  .droite {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.25rem 0.6rem;
    margin-left: auto;
    min-width: 0;
  }
  .biblio {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.25rem 0.6rem;
    min-width: 0;
  }
  .biblio-row {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    font-size: 0.78rem;
    min-width: 0;
    max-width: 22rem;
  }
  .biblio-nom {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }
  /* Bouton « Transformer en widget ! » — recette « verre Aero HUD » compacte
     variante bordeaux (--dash-danger, comme .header.etat-erreur Toolbar). */
  .action.danger {
    --btn-tint: var(--dash-danger);
    --btn-border: linear-gradient(var(--dash-danger), var(--dash-danger));
    background:
      var(--btn-surface) padding-box,
      var(--btn-border) border-box;
    color: var(--dash-danger);
    border: 1px solid transparent;
    box-shadow: var(--btn-inset);
    padding: 0.15rem 0.5rem;
    font: inherit;
    font-size: 0.78rem;
    cursor: pointer;
    white-space: nowrap;
    flex-shrink: 0;
    transition: box-shadow 0.2s ease, background 0.2s ease;
  }
  .action.danger:hover {
    background:
      var(--btn-surface-hover) padding-box,
      var(--btn-border) border-box;
    box-shadow: var(--btn-inset-hover), var(--dash-glow-danger);
  }
  .action.danger:active {
    background:
      linear-gradient(180deg, var(--fond-controle) 0%, color-mix(in srgb, var(--btn-tint) 8%, var(--fond-controle)) 100%) padding-box,
      var(--btn-border) border-box;
    box-shadow: inset 0 2px 4px rgba(0, 0, 0, 0.4);
  }
</style>
