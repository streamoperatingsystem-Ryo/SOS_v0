<script lang="ts">
  // CadresModal — modale galerie de cadres SVG (ouverte depuis Toolbar).
  // mode = "widget" (cadreWidget) ou "app" (cadreApp).
  import { cadreModalOpen } from "../stores/ui";
  import {
    sceneStore,
    mettreAJourCadreScene,
    mettreAJourCadreApp,
  } from "../stores/scene";
  import { REGISTRE_CADRES, trouverCadre } from "../cadres/registre";
  import CadreSVG from "./CadreSVG.svelte";

  let { mode }: { mode: "widget" | "app" } = $props();

  const THUMB_W = 120;
  const THUMB_H = 68;

  // Valeurs courantes selon le mode.
  let cadreCourant = $derived(
    mode === "widget" ? $sceneStore.cadreWidget : $sceneStore.cadreApp
  );
  let styleCourant = $derived(cadreCourant?.style ?? "carre");
  let varianteCourante = $derived(cadreCourant?.variante ?? null);
  let strokeWidthCourant = $derived(cadreCourant?.strokeWidth ?? 4);
  let couleurCourante = $derived(cadreCourant?.couleur ?? "#ffffff");
  let couleurFinCourante = $derived(cadreCourant?.couleurFin ?? "#000000");
  let actifCourant = $derived(cadreCourant?.actif ?? false);

  // Cadre sélectionné dans le registre.
  let cadreRegistre = $derived(trouverCadre(styleCourant));
  let estPersonnalisable = $derived(cadreRegistre?.personnalisable ?? false);

  // Couleur représentative d'une variante (pour les pastilles).
  function getVarianteColor(cadreId: string, varianteId: string): string {
    const COLORS: Record<string, Record<string, string>> = {
      story: { pink: "#ff4d8d", blue: "#4d88ff", purple: "#8c4dff", green: "#4dff88", orange: "#ff884d", yellow: "#ffd700" },
      cyberpunk: { defaut: "#00f0ff", "vert-magenta": "#39ff14", "orange-cyan": "#ff8800", "violet-cyan": "#8c4dff", "rouge-cyan": "#ff2244" },
      "thin-line": { graphite: "#2d2d2d", argent: "#a0a0a0", "blanc-pure": "#ffffff" },
      hexagon: { ambre: "#f59e0b", azure: "#0ea5e9", emerald: "#10b981", violet: "#8b5cf6", steel: "#64748b" },
    };
    const cadreColors = COLORS[cadreId];
    if (cadreColors && cadreColors[varianteId]) return cadreColors[varianteId];
    return "#67e8f9";
  }

  // --- Handlers ---
  function selectionnerCadre(cadreId: string) {
    const cadre = trouverCadre(cadreId);
    const newVariante = cadre?.variantes && cadre.variantes.length > 0 ? cadre.variantes[0].id : undefined;
    const patch: Record<string, unknown> = { style: cadreId, actif: true };
    if (newVariante) patch.variante = newVariante;
    if (mode === "widget") {
      mettreAJourCadreScene(patch);
    } else {
      mettreAJourCadreApp(patch);
    }
  }

  function selectionnerVariante(varianteId: string) {
    if (mode === "widget") {
      mettreAJourCadreScene({ variante: varianteId, actif: true });
    } else {
      mettreAJourCadreApp({ variante: varianteId, actif: true });
    }
  }

  function changerStrokeWidth(val: number) {
    const n = Math.max(1, Math.min(20, Number(val)));
    if (mode === "widget") {
      mettreAJourCadreScene({ strokeWidth: n });
    } else {
      mettreAJourCadreApp({ strokeWidth: n });
    }
  }

  function changerCouleur(val: string) {
    if (mode === "widget") {
      mettreAJourCadreScene({ couleur: val });
    } else {
      mettreAJourCadreApp({ couleur: val });
    }
  }

  function changerCouleurFin(val: string) {
    if (mode === "widget") {
      mettreAJourCadreScene({ couleurFin: val });
    } else {
      mettreAJourCadreApp({ couleurFin: val });
    }
  }

  function toggleActif() {
    if (mode === "widget") {
      mettreAJourCadreScene({ actif: !actifCourant });
    } else {
      mettreAJourCadreApp({ actif: !actifCourant });
    }
  }

  function fermer() {
    cadreModalOpen.set(null);
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      fermer();
    }
  }
</script>

<svelte:window onkeydown={onKeydown} />

{#if $cadreModalOpen}
  <div class="overlay" onclick={fermer} role="presentation">
    <div
      class="modal"
      onclick={(e) => e.stopPropagation()}
      role="dialog"
      aria-modal="true"
      tabindex="-1"
    >
      <!-- Header -->
      <div class="modal-header">
        <h2>
          {mode === "widget" ? "Cadre des widgets" : "Cadre de l'application"}
        </h2>
        <button class="close-btn" onclick={fermer} aria-label="Fermer">✕</button>
      </div>

      <!-- Corps : grille + panneau config -->
      <div class="modal-body">
        <!-- Grille de miniatures -->
        <div class="grille">
          {#each REGISTRE_CADRES as cadre (cadre.id)}
            <div class="cadre-item">
              <button
                class="thumb {styleCourant === cadre.id ? 'selectionne' : ''}"
                onclick={() => selectionnerCadre(cadre.id)}
              >
                <div class="thumb-vignette">
                  <CadreSVG
                    style={cadre.id}
                    variante={cadre.variantes?.[0]?.id}
                    largeur={THUMB_W}
                    hauteur={THUMB_H}
                    strokeWidth={3}
                    idCadre="thumb-{cadre.id}"
                  />
                </div>
                <span class="thumb-nom">{cadre.nom}</span>
              </button>
              {#if cadre.variantes && cadre.variantes.length > 1}
                <div class="variantes">
                  {#each cadre.variantes as v (v.id)}
                    <button
                      class="variante {styleCourant === cadre.id && varianteCourante === v.id ? 'actif' : ''}"
                      title={v.nom}
                      style="background-color: {getVarianteColor(cadre.id, v.id)};"
                      onclick={() => selectionnerVariante(v.id)}
                    ></button>
                  {/each}
                </div>
              {/if}
            </div>
          {/each}
        </div>

        <!-- Panneau config -->
        <div class="config">
          <div class="config-nom">{cadreRegistre?.nom ?? "Aucun cadre"}</div>

          <button class="toggle {actifCourant ? 'actif' : 'inactif'}" onclick={toggleActif}>
            {actifCourant ? "Cadre activé" : "Cadre désactivé"}
          </button>

          <!-- Épaisseur -->
          <div class="config-section">
            <span class="config-label">Épaisseur</span>
            <div class="config-row">
              <input
                type="range"
                min="1"
                max="20"
                step="1"
                class="range"
                value={strokeWidthCourant}
                oninput={(e) => changerStrokeWidth(parseFloat((e.currentTarget as HTMLInputElement).value))}
              />
              <span class="config-valeur">{strokeWidthCourant}px</span>
            </div>
          </div>

          <!-- Couleurs (si personnalisable) -->
          <div class="config-section {estPersonnalisable ? '' : 'desactive'}">
            <span class="config-label">Dégradé</span>
            <div class="config-row">
              <input
                type="color"
                class="color"
                value={couleurCourante}
                oninput={(e) => changerCouleur((e.currentTarget as HTMLInputElement).value)}
              />
              <span class="config-texte">Début</span>
              <span class="config-hex">{couleurCourante}</span>
            </div>
            <div class="config-row">
              <input
                type="color"
                class="color"
                value={couleurFinCourante}
                oninput={(e) => changerCouleurFin((e.currentTarget as HTMLInputElement).value)}
              />
              <span class="config-texte">Fin</span>
              <span class="config-hex">{couleurFinCourante}</span>
            </div>
            {#if !estPersonnalisable}
              <div class="config-info">Couleurs non modifiables (preset)</div>
            {/if}
          </div>

          <!-- Aperçu -->
          <div class="config-section">
            <span class="config-label">Aperçu</span>
            <div class="apercu">
              <CadreSVG
                style={styleCourant}
                variante={varianteCourante ?? undefined}
                largeur={200}
                hauteur={112}
                strokeWidth={strokeWidthCourant}
                couleur={couleurCourante}
                couleurFin={couleurFinCourante}
                idCadre="apercu-{mode}"
              />
            </div>
          </div>
        </div>
      </div>
    </div>
  </div>
{/if}

<style>
  .overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.6);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 10000;
  }
  .modal {
    background: var(--fond);
    color: var(--texte);
    border: 1px solid var(--texte);
    display: flex;
    flex-direction: column;
    width: 90vw;
    max-width: 720px;
    max-height: 85vh;
    overflow: hidden;
  }
  .modal-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0.75rem 1rem;
    border-bottom: 1px solid var(--texte);
  }
  h2 {
    font-size: 1rem;
    margin: 0;
    font-weight: 600;
  }
  .close-btn {
    background: var(--fond);
    color: var(--texte);
    border: 1px solid var(--texte);
    font-size: 0.85rem;
    cursor: pointer;
    padding: 0.2rem 0.5rem;
    line-height: 1;
  }
  .close-btn:hover {
    background: var(--texte);
    color: var(--fond);
  }
  .modal-body {
    display: flex;
    flex: 1;
    overflow: hidden;
  }
  /* Grille de miniatures */
  .grille {
    flex: 1;
    overflow-y: auto;
    padding: 0.5rem;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(140px, 1fr));
    gap: 0.4rem;
    align-content: start;
  }
  .cadre-item {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.2rem;
  }
  .thumb {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.3rem;
    padding: 0.4rem;
    border: 1px solid var(--texte);
    background: var(--fond);
    cursor: pointer;
    width: 100%;
  }
  .thumb:hover {
    background: var(--texte);
    color: var(--fond);
  }
  .thumb.selectionne {
    background: var(--texte);
    color: var(--fond);
  }
  .thumb-vignette {
    width: 120px;
    height: 68px;
    position: relative;
    overflow: hidden;
    background: var(--fond);
  }
  .thumb-nom {
    font-size: 0.7rem;
    text-align: center;
    max-width: 120px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .variantes {
    display: flex;
    flex-wrap: wrap;
    gap: 3px;
    justify-content: center;
    width: 120px;
    padding: 3px;
    border: 1px solid var(--texte);
    background: var(--fond);
  }
  .variante {
    width: 12px;
    height: 12px;
    border: 1px solid var(--texte);
    cursor: pointer;
    padding: 0;
  }
  .variante:hover {
    opacity: 0.7;
  }
  .variante.actif {
    border-width: 2px;
  }
  /* Panneau config */
  .config {
    width: 220px;
    flex-shrink: 0;
    padding: 0.5rem;
    border-left: 1px solid var(--texte);
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
  }
  .config-nom {
    font-size: 0.9rem;
    font-weight: 700;
    text-align: center;
    padding: 0.3rem 0;
    border-bottom: 1px solid var(--texte);
  }
  .toggle {
    padding: 0.4rem 0.6rem;
    border: 1px solid var(--texte);
    font-size: 0.8rem;
    font-weight: 600;
    cursor: pointer;
    text-align: center;
    background: var(--fond);
    color: var(--texte);
  }
  .toggle:hover {
    background: var(--texte);
    color: var(--fond);
  }
  .toggle.actif {
    background: var(--texte);
    color: var(--fond);
  }
  .config-section {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
  }
  .config-section.desactive {
    opacity: 0.4;
    pointer-events: none;
  }
  .config-label {
    font-size: 0.65rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    opacity: 0.6;
  }
  .config-row {
    display: flex;
    align-items: center;
    gap: 0.3rem;
  }
  .range {
    flex: 1;
    height: 14px;
    cursor: pointer;
    accent-color: var(--texte);
    background: var(--fond);
  }
  .config-valeur {
    font-size: 0.7rem;
    opacity: 0.8;
    font-variant-numeric: tabular-nums;
    width: 32px;
    text-align: right;
  }
  .color {
    width: 28px;
    height: 24px;
    border: 1px solid var(--texte);
    background: transparent;
    cursor: pointer;
    flex-shrink: 0;
    padding: 0;
  }
  .config-texte {
    font-size: 0.7rem;
    opacity: 0.7;
  }
  .config-hex {
    font-size: 0.65rem;
    opacity: 0.5;
    font-family: monospace;
  }
  .config-info {
    font-size: 0.65rem;
    opacity: 0.5;
    font-style: italic;
  }
  .apercu {
    width: 200px;
    height: 112px;
    position: relative;
    overflow: hidden;
    background: var(--fond);
    border: 1px solid var(--texte);
  }
</style>
