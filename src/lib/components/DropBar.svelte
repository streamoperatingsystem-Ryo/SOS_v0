<script lang="ts">
  // Bandeau drop médias : zone « Glisser vos .PNG ici ou images/video »
  // (centrée) + liste des fichiers droppés (noms seuls) à droite.
  // Pleine largeur de .canvas-col, rendu entre SceneBar et Canvas
  // (App.svelte). Le toggle « Transformer en widget / flottant » vit
  // maintenant dans la carte d'édition (CarteEdition, onglet Contenu).
  //
  // Le drop passe par l'event Tauri (dragDropEnabled=true intercepte le HTML5
  // drop) — aucun handler dragover/drop DOM. La position DragDropEvent est en
  // pixels PHYSIQUES ; getBoundingClientRect est en CSS px → /dndScale.
  // Hit-test = rect du bandeau entier (label + liste) : drop sur le canvas ou
  // la sidebar = aucun import.
  import { bibliothequeMedias, ajouterALaBibliotheque } from "../stores/scene";
  import { onMount, onDestroy } from "svelte";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { t } from "../stores/langue";

  let biblio = $derived($bibliothequeMedias);
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
  <span class="drop-label">{$t("dropBar.glisser")}</span>
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
  </div>
</div>

<style>
  /* Bandeau horizontal pleine largeur, bordure fine discrète. Hauteur
     FIXE (2.2rem) — jamais de variation : vide, avec bibliothèque ou bouton
     « Transformer », la barre garde exactement la même hauteur. Le surplus
     de fichiers défile horizontalement dans le groupe de droite (.droite),
     plus verticalement dans la barre. */
  /* Grid 3 colonnes (1fr | auto | 1fr) : le label reste DANS le flux et est
     centré dans la barre entière — les deux colonnes latérales ont toujours
     la même largeur, quel que soit le contenu du groupe de droite.
     minmax(0,1fr) autorise la colonne droite à se comprimer (sinon auto =
     taille du contenu) ; son contenu défile horizontalement. */
  .drop-bar {
    flex-shrink: 0;
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto minmax(0, 1fr);
    align-items: center;
    gap: 0.25rem 0.6rem;
    padding: 0.3rem 0.6rem;
    /* Pointillés du cadre utilisateur : même motif 2px dashed que
       .carte-edition (app.css), mais BICOLORE — chaque tiret suit le
       dégradé du cadre (--cadre-couleur → --cadre-couleur-fin). Rendu
       via 4 bandes repeating-linear-gradient : un border:dashed ne prend
       qu'UNE couleur et border-image ignore dashed. Si une extrémité du
       dégradé tombe au noir, l'autre couleur reste visible. Fallbacks =
       accents du thème (mêmes que .section::before, Toolbar.svelte).
       border:solid transparent réserve la géométrie 2px des cartes. */
    border: 2px solid transparent;
    background-origin: border-box;
    background-image:
      repeating-linear-gradient(90deg, var(--cadre-couleur, var(--accent-orange)) 0px, var(--cadre-couleur-fin, var(--accent-violet)) 8px, transparent 8px, transparent 16px),
      repeating-linear-gradient(90deg, var(--cadre-couleur, var(--accent-orange)) 0px, var(--cadre-couleur-fin, var(--accent-violet)) 8px, transparent 8px, transparent 16px),
      repeating-linear-gradient(0deg, var(--cadre-couleur, var(--accent-orange)) 0px, var(--cadre-couleur-fin, var(--accent-violet)) 8px, transparent 8px, transparent 16px),
      repeating-linear-gradient(0deg, var(--cadre-couleur, var(--accent-orange)) 0px, var(--cadre-couleur-fin, var(--accent-violet)) 8px, transparent 8px, transparent 16px);
    background-repeat: repeat-x, repeat-x, repeat-y, repeat-y;
    background-size: 16px 2px, 16px 2px, 2px 16px, 2px 16px;
    background-position: 0 0, 0 100%, 0 0, 100% 0;
    /* Fond de la barre : transparent — l'ambiance transparaît entre les
       tirets (seules les 4 bandes de bord sont peintes). */
    font-size: 0.82rem;
    color: var(--texte);
    opacity: 0.8;
    height: 2.2rem;
    overflow: hidden;
    transition: border-color 0.15s ease, box-shadow 0.15s ease, opacity 0.15s ease;
  }
  /* .hover = curseur Tauri DANS la zone (pas un hover CSS — la position
     physique de l'event drop est comparée au rect de l'élément). La
     bordure reste transparente : une ligne pleine recouvrirait les tirets
     (et pourrait être le bout SOMBRE du dégradé). Renfort = glow des 2
     couleurs du cadre + opacity pleine. */
  .drop-bar.hover {
    border-color: transparent;
    box-shadow:
      var(--glow-ui),
      0 0 10px color-mix(in srgb, var(--cadre-couleur-fin, var(--accent-violet)) 50%, transparent);
    opacity: 1;
  }
  /* Label : colonne centrale de la grid — centré dans la barre entière,
     même quand le groupe de droite (biblio + bouton) est présent. */
  .drop-label {
    grid-column: 2;
    justify-self: center;
    white-space: nowrap;
  }
  /* Groupe de droite (noms + bouton Transformer) : colonne 3, collé au bord
     droit du bandeau — le bouton reste à droite même quand la liste est
     vide. nowrap + scroll horizontal : la hauteur de la barre est fixe,
     les noms longs gardent leur ellipsis, le surplus défile à l'horizontale. */
  .droite {
    grid-column: 3;
    justify-self: end;
    display: flex;
    flex-wrap: nowrap;
    align-items: center;
    gap: 0.25rem 0.6rem;
    min-width: 0;
    overflow-x: auto;
    overflow-y: hidden;
    scrollbar-width: thin;
  }
  .biblio {
    display: flex;
    flex-wrap: nowrap;
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
</style>
