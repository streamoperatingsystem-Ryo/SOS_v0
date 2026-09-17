<script lang="ts">
  // CadresModal — modale galerie de cadres SVG (ouverte depuis Toolbar).
  // Deux onglets : "widget" (cadre autour de chaque widget) et "app" (bord de
  // l'application = autour de toute la scène). L'onglet courant = le store
  // cadreModalOpen (source unique) : le prop mode vient de App.svelte, et la
  // bascule d'onglet écrit dans le store (la modale reste ouverte tant que
  // la valeur est non-null).
  import { cadreModalOpen } from "../stores/ui";
  import {
    sceneStore,
    mettreAJourCadreScene,
    mettreAJourCadreApp,
  } from "../stores/scene";
  import { REGISTRE_CADRES, trouverCadre, getVarianteColor } from "../cadres/registre";
  import CadreSVG from "./CadreSVG.svelte";
  import Modal from "./Modal.svelte";

  let { mode }: { mode: "widget" | "app" } = $props();

  /// Bascule d'onglet : écrit dans le store (source unique).
  function changerOnglet(m: "widget" | "app") {
    cadreModalOpen.set(m);
  }

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
  let gradientAngleCourant = $derived(cadreCourant?.gradientAngle ?? 135);
  let actifCourant = $derived(cadreCourant?.actif ?? false);

  // Cadre sélectionné dans le registre.
  let cadreRegistre = $derived(trouverCadre(styleCourant));
  let estPersonnalisable = $derived(cadreRegistre?.personnalisable ?? false);

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
    // Synchronise la couleur sur les deux cadres (widget + app).
    mettreAJourCadreScene({ couleur: val });
    mettreAJourCadreApp({ couleur: val });
  }

  function changerCouleurFin(val: string) {
    mettreAJourCadreScene({ couleurFin: val });
    mettreAJourCadreApp({ couleurFin: val });
  }

  function changerGradientAngle(val: number) {
    const n = Math.max(0, Math.min(360, Number(val)));
    mettreAJourCadreScene({ gradientAngle: n });
    mettreAJourCadreApp({ gradientAngle: n });
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
</script>

{#if $cadreModalOpen}
  <Modal
    title="Vos cadres"
    onClose={fermer}
    maxWidth="720px"
    hint={mode === "widget"
      ? "Ce cadre entoure chaque widget (médias, chat, caméra…) individuellement. Choisissez un style ci-contre, ajustez-le, puis activez-le."
      : "Ce cadre entoure le bord de toute la scène diffusée sur le stream. Choisissez un style ci-contre, ajustez-le, puis activez-le."}
  >
    {#snippet headerExtra()}
      <button
        class="onglet {mode === 'widget' ? 'actif' : ''}"
        onclick={() => changerOnglet("widget")}
      >
        Cadre des widgets
      </button>
      <button
        class="onglet {mode === 'app' ? 'actif' : ''}"
        onclick={() => changerOnglet("app")}
      >
        Bord de l'application
      </button>
    {/snippet}

    <!-- Corps : grille + panneau config -->
    <div class="modal-body-content">
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
              class="modal-range"
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
          <div class="config-row">
            <input
              type="range"
              min="0"
              max="360"
              step="1"
              class="modal-range"
              value={gradientAngleCourant}
              oninput={(e) => changerGradientAngle(parseFloat((e.currentTarget as HTMLInputElement).value))}
            />
            <span class="config-valeur">{gradientAngleCourant}°</span>
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
              gradientAngle={gradientAngleCourant}
              idCadre="apercu-{mode}"
            />
          </div>
        </div>
      </div>
    </div>
  </Modal>
{/if}

<style>
  /* Onglets de la modale : cible du cadre (widgets / bord de l'application).
     Rendus via le slot headerExtra du wrapper Modal. */
  .onglet {
    background: var(--btn-surface);
    box-shadow: var(--btn-inset);
    color: var(--texte);
    border: 1px solid var(--bordure);
    border-radius: var(--rayon-petit);
    padding: 0.3rem 0.7rem;
    font: inherit;
    font-size: 0.8rem;
    cursor: pointer;
    transition: background 0.15s ease, box-shadow 0.15s ease, border-color 0.15s ease;
  }
  .onglet:hover {
    background: var(--btn-surface-hover);
    box-shadow: var(--btn-inset-hover);
    border-color: var(--accent-violet);
  }
  .onglet.actif {
    background:
      linear-gradient(
        180deg,
        color-mix(in srgb, var(--accent-violet) 25%, var(--fond-controle)) 0%,
        color-mix(in srgb, var(--accent-violet) 12%, var(--fond-controle)) 100%
      );
    color: var(--texte);
    border-color: var(--accent-violet);
  }
  .modal-body-content {
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
    border: 1px solid var(--bordure);
    background: var(--btn-surface);
    box-shadow: var(--btn-inset);
    cursor: pointer;
    width: 100%;
    transition: background 0.15s ease, box-shadow 0.15s ease, border-color 0.15s ease;
  }
  .thumb:hover {
    background: var(--btn-surface-hover);
    box-shadow: var(--btn-inset-hover);
    border-color: var(--accent-violet);
  }
  .thumb.selectionne {
    background:
      linear-gradient(
        180deg,
        color-mix(in srgb, var(--accent-violet) 25%, var(--fond-controle)) 0%,
        color-mix(in srgb, var(--accent-violet) 12%, var(--fond-controle)) 100%
      );
    color: var(--texte);
    border-color: var(--accent-violet);
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
    border: 1px solid var(--bordure);
    background: var(--fond);
  }
  .variante {
    width: 12px;
    height: 12px;
    border: 1px solid var(--bordure);
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
    border-left: 1px solid var(--bordure);
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
    border-bottom: 1px solid var(--bordure);
  }
  .toggle {
    padding: 0.4rem 0.6rem;
    border: 1px solid var(--bordure);
    font-size: 0.8rem;
    font-weight: 600;
    cursor: pointer;
    text-align: center;
    background: var(--btn-surface);
    box-shadow: var(--btn-inset);
    color: var(--texte);
    transition: background 0.15s ease, box-shadow 0.15s ease, border-color 0.15s ease;
  }
  .toggle:hover {
    background: var(--btn-surface-hover);
    box-shadow: var(--btn-inset-hover);
    border-color: var(--accent-violet);
  }
  .toggle.actif {
    background:
      linear-gradient(
        180deg,
        color-mix(in srgb, var(--accent-violet) 25%, var(--fond-controle)) 0%,
        color-mix(in srgb, var(--accent-violet) 12%, var(--fond-controle)) 100%
      );
    color: var(--texte);
    border-color: var(--accent-violet);
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
    border: 1px solid var(--bordure);
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
    border: 1px solid var(--bordure);
  }
</style>
