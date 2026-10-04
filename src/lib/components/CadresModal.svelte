<script lang="ts">
  // CadresModal — modale galerie du cadre SVG unifié (ouverte depuis Toolbar).
  // UN SEUL système depuis 2026-09-26 : une seule CadreConfig (cadreWidget)
  // alimente en même temps le cadre de chaque widget ET le bord du canvas —
  // plus d'onglets widget/app, plus de config divergente possible.
  import { cadreModalOpen } from "../stores/ui";
  import { sceneStore, mettreAJourCadre } from "../stores/scene";
  import { REGISTRE_CADRES, trouverCadre } from "../cadres/registre";
  import { t } from "../stores/langue";
  import CadreSVG from "./CadreSVG.svelte";
  import Modal from "./Modal.svelte";

  const THUMB_W = 120;
  const THUMB_H = 68;

  // Valeurs courantes du cadre unifié (champ canonique cadreWidget).
  let cadreCourant = $derived($sceneStore.cadreWidget);
  let styleCourant = $derived(cadreCourant?.style ?? "carre");
  let strokeWidthCourant = $derived(cadreCourant?.strokeWidth ?? 4);
  let couleurCourante = $derived(cadreCourant?.couleur ?? "#ffffff");
  let couleurFinCourante = $derived(cadreCourant?.couleurFin ?? "#000000");
  let gradientAngleCourant = $derived(cadreCourant?.gradientAngle ?? 135);
  let actifCourant = $derived(cadreCourant?.actif ?? false);
  // Portée : le cadre s'applique aussi au bord du canvas (défaut true).
  let bordActifCourant = $derived(cadreCourant?.bordActif ?? true);

  // Cadre sélectionné dans le registre.
  let cadreRegistre = $derived(trouverCadre(styleCourant));

  // Noms de cadres traduits ($t scopé interdit dans un {#each} → map par id).
  let nomsCadres = $derived.by(() => {
    const tr = $t;
    return Object.fromEntries(
      REGISTRE_CADRES.map((c) => [c.id, tr("cadres.nom." + c.id)])
    ) as Record<string, string>;
  });

  // --- Handlers ---
  function selectionnerCadre(cadreId: string) {
    mettreAJourCadre({ style: cadreId, actif: true });
  }

  function changerStrokeWidth(val: number) {
    const n = Math.max(1, Math.min(20, Number(val)));
    mettreAJourCadre({ strokeWidth: n });
  }

  function changerCouleur(val: string) {
    mettreAJourCadre({ couleur: val });
  }

  function changerCouleurFin(val: string) {
    mettreAJourCadre({ couleurFin: val });
  }

  function changerGradientAngle(val: number) {
    const n = Math.max(0, Math.min(360, Number(val)));
    mettreAJourCadre({ gradientAngle: n });
  }

  function toggleActif() {
    mettreAJourCadre({ actif: !actifCourant });
  }

  function toggleBord() {
    mettreAJourCadre({ bordActif: !bordActifCourant });
  }

  function fermer() {
    cadreModalOpen.set(false);
  }
</script>

{#if $cadreModalOpen}
  <Modal
    title={$t("cadresModal.titre")}
    onClose={fermer}
    maxWidth="720px"
    hint={$t("cadresModal.hint")}
  >
    {#snippet headerExtra()}
      <span class="sous-titre">{$t("cadresModal.sousTitre")}</span>
    {/snippet}

    <!-- Corps : grille + panneau config -->
    <div class="modal-body-content">
      <!-- Grille de miniatures (rendues avec les couleurs courantes) -->
      <div class="grille">
        {#each REGISTRE_CADRES as cadre (cadre.id)}
          <button
            class="thumb {styleCourant === cadre.id ? 'selectionne' : ''}"
            onclick={() => selectionnerCadre(cadre.id)}
          >
            <div class="thumb-vignette">
              <CadreSVG
                style={cadre.id}
                largeur={THUMB_W}
                hauteur={THUMB_H}
                strokeWidth={3}
                couleur={couleurCourante}
                couleurFin={couleurFinCourante}
                gradientAngle={gradientAngleCourant}
                idCadre="thumb-{cadre.id}"
              />
            </div>
            <span class="thumb-nom">{nomsCadres[cadre.id] ?? cadre.nom}</span>
          </button>
        {/each}
      </div>

      <!-- Panneau config -->
      <div class="config">
        <div class="config-nom">{cadreRegistre ? (nomsCadres[cadreRegistre.id] ?? cadreRegistre.nom) : $t("cadresModal.aucun")}</div>

        <button class="toggle {actifCourant ? 'actif' : 'inactif'}" onclick={toggleActif}>
          {actifCourant ? $t("cadresModal.active") : $t("cadresModal.desactive")}
        </button>

        <button class="toggle {bordActifCourant ? 'actif' : 'inactif'}" onclick={toggleBord} disabled={!actifCourant}>
          {bordActifCourant ? $t("cadresModal.bordActive") : $t("cadresModal.bordDesactive")}
        </button>

        <!-- Épaisseur -->
        <div class="config-section">
          <span class="config-label">{$t("cadresModal.epaisseur")}</span>
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

        <!-- Couleurs (tous les cadres sont personnalisables) -->
        <div class="config-section">
          <span class="config-label">{$t("cadresModal.degrade")}</span>
          <div class="config-row">
            <input
              type="color"
              class="color"
              value={couleurCourante}
              oninput={(e) => changerCouleur((e.currentTarget as HTMLInputElement).value)}
            />
            <span class="config-texte">{$t("commun.debut")}</span>
            <span class="config-hex">{couleurCourante}</span>
          </div>
          <div class="config-row">
            <input
              type="color"
              class="color"
              value={couleurFinCourante}
              oninput={(e) => changerCouleurFin((e.currentTarget as HTMLInputElement).value)}
            />
            <span class="config-texte">{$t("commun.fin")}</span>
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
        </div>

        <!-- Aperçu -->
        <div class="config-section">
          <span class="config-label">{$t("commun.apercu")}</span>
          <div class="apercu">
            <CadreSVG
              style={styleCourant}
              largeur={200}
              hauteur={112}
              strokeWidth={strokeWidthCourant}
              couleur={couleurCourante}
              couleurFin={couleurFinCourante}
              gradientAngle={gradientAngleCourant}
              idCadre="apercu"
            />
          </div>
        </div>
      </div>
    </div>
  </Modal>
{/if}

<style>
  /* Sous-titre dans le header (slot headerExtra du wrapper Modal) : rappelle
     que le cadre unique s'applique aux widgets ET au bord du canvas. */
  .sous-titre {
    font-size: 0.8rem;
    opacity: 0.7;
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
  .thumb {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.3rem;
    padding: 0.4rem;
    border: 1px solid var(--bordure-ui);
    background: var(--btn-surface);
    box-shadow: var(--btn-inset);
    cursor: pointer;
    width: 100%;
    transition: background 0.15s ease, box-shadow 0.15s ease, border-color 0.15s ease;
  }
  .thumb:hover {
    background: var(--btn-surface-hover);
    box-shadow: var(--btn-inset-hover);
    border-color: var(--accent-ui);
  }
  .thumb.selectionne {
    background:
      linear-gradient(
        180deg,
        color-mix(in srgb, var(--accent-ui) 25%, var(--fond-controle)) 0%,
        color-mix(in srgb, var(--accent-ui) 12%, var(--fond-controle)) 100%
      );
    color: var(--texte);
    border-color: var(--accent-ui);
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
  /* Panneau config */
  .config {
    width: 220px;
    flex-shrink: 0;
    padding: 0.5rem;
    border-left: 1px solid var(--bordure-ui);
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
    border-bottom: 1px solid var(--bordure-ui);
  }
  .toggle {
    padding: 0.4rem 0.6rem;
    border: 1px solid var(--bordure-ui);
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
    border-color: var(--accent-ui);
  }
  .toggle.actif {
    background:
      linear-gradient(
        180deg,
        color-mix(in srgb, var(--accent-ui) 25%, var(--fond-controle)) 0%,
        color-mix(in srgb, var(--accent-ui) 12%, var(--fond-controle)) 100%
      );
    color: var(--texte);
    border-color: var(--accent-ui);
  }
  .toggle:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
  .config-section {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
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
    border: 1px solid var(--bordure-ui);
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
  .apercu {
    width: 200px;
    height: 112px;
    position: relative;
    overflow: hidden;
    background: var(--fond);
    border: 1px solid var(--bordure-ui);
  }
</style>
