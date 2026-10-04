<script lang="ts">
  // PresetsModal — galerie de scènes modèles embarquées (« tout prêt » pour
  // le user lambda). Deux modes (store presetsModalOpen, pattern cadreModalOpen) :
  // - "creation" (bouton « + Scène ») : champ nom + card « Page vierge » +
  //   cards presets. Chaque preset crée une NOUVELLE scène (l'existante
  //   jamais touchée) ; « Page vierge » = comportement historique (createScene).
  // - "onboarding" (premier lancement) : choix de la scène de DÉPART —
  //   initialise la scène courante (vide au 1er boot) via Rust (remplace le
  //   contenu + renomme), puis marque l'onboarding fait. Fermer sans choisir
  //   (✕ / Escape / overlay) = page vierge → onboarding marqué fait aussi.
  import { presetsModalOpen } from "../stores/ui";
  import {
    presetsStore,
    chargerPresets,
    creerSceneDepuisPreset,
    initialiserSceneDepuisPreset,
  } from "../stores/presets";
  import { scenesIndexStore, createScene } from "../stores/scenes";
  import { tauri } from "../tauri";
  import Modal from "./Modal.svelte";
  import { t } from "../stores/langue";

  let { mode }: { mode: "creation" | "onboarding" } = $props();

  // Charge la galerie à l'ouverture (idempotent — App.svelte le fait aussi
  // au 1er boot pour l'onboarding).
  void chargerPresets();

  // Nom saisi (mode création). Vide → défaut selon le choix : nom du preset,
  // ou « Scène N » pour la page vierge (comportement historique du bouton +).
  let nomSaisi = $state("");
  let defautVierge = $derived($t("presetsModal.nomDefaut", { n: $scenesIndexStore.length + 1 }));

  // Application en vol (désactive les cards pendant l'invoke).
  let occupe = $state(false);

  function fermer() {
    presetsModalOpen.set(null);
  }

  /// Fermeture à l'onboarding sans choix = page vierge → marquer fait
  /// (sinon la modale reviendrait à chaque boot).
  async function fermerOnboarding() {
    try {
      await tauri.onboardingMarquerFait();
    } catch {
      // Non-fatal : la modale réapparaîtra au prochain boot, pas grave.
    }
    fermer();
  }

  async function choisirPreset(presetId: string, presetNom: string) {
    if (occupe) return;
    occupe = true;
    try {
      if (mode === "onboarding") {
        await initialiserSceneDepuisPreset(presetId);
        fermer();
      } else {
        const nom = nomSaisi.trim() || presetNom;
        const ok = await creerSceneDepuisPreset(presetId, nom);
        if (ok) fermer();
      }
    } finally {
      occupe = false;
    }
  }

  async function choisirVierge() {
    if (occupe) return;
    if (mode === "onboarding") {
      await fermerOnboarding();
      return;
    }
    occupe = true;
    try {
      await createScene(nomSaisi.trim() || defautVierge);
      fermer();
    } finally {
      occupe = false;
    }
  }

  // Icônes des cards (tracés SVG internes, stroke currentColor — même style
  // que les .ico de l'app). Fallback : carré minimaliste.
  const ICONES: Record<string, string> = {
    gaming:
      '<rect x="2" y="7" width="20" height="10" rx="5"/><path d="M7 11v2"/><path d="M6 12h2"/><circle cx="15.5" cy="12" r="0.6" fill="currentColor" stroke="none"/><circle cx="18.5" cy="12" r="0.6" fill="currentColor" stroke="none"/>',
    "just-chatting":
      '<path d="M21 11.5a8.38 8.38 0 0 1-.9 3.8 8.5 8.5 0 0 1-7.6 4.7 8.38 8.38 0 0 1-3.8-.9L3 21l1.9-5.7a8.38 8.38 0 0 1-.9-3.8 8.5 8.5 0 0 1 4.7-7.6 8.38 8.38 0 0 1 3.8-.9h.5a8.48 8.48 0 0 1 8 8v.5z"/>',
    speedrun:
      '<circle cx="12" cy="13" r="8"/><path d="M12 13l3-3"/><path d="M9 2h6"/><path d="M12 2v2"/>',
    minimal: '<rect x="4" y="4" width="16" height="16" rx="3"/>',
  };
  const ICONE_VIERGE =
    '<line x1="12" y1="5" x2="12" y2="19"/><line x1="5" y1="12" x2="19" y2="12"/>';

  function iconePour(id: string): string {
    return ICONES[id] ?? ICONES.minimal;
  }
</script>

<Modal
  title={mode === "onboarding" ? $t("presetsModal.titreOnboarding") : $t("presetsModal.titreCreation")}
  onClose={mode === "onboarding" ? fermerOnboarding : fermer}
  maxWidth="760px"
  hint={mode === "onboarding"
    ? $t("presetsModal.hintOnboarding")
    : $t("presetsModal.hintCreation")}
>
  {#if mode === "creation"}
    <div class="nom-row">
      <label for="preset-nom">{$t("commun.nom")}</label>
      <input
        id="preset-nom"
        type="text"
        bind:value={nomSaisi}
        placeholder={defautVierge}
        maxlength="40"
        spellcheck="false"
      />
    </div>
  {/if}

  <div class="grille">
    {#if mode === "creation"}
      <button class="preset-card" onclick={choisirVierge} disabled={occupe}>
        <svg class="preset-ico" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
          {@html ICONE_VIERGE}
        </svg>
        <span class="preset-nom">{$t("presetsModal.pageVierge")}</span>
        <span class="preset-desc">{$t("presetsModal.pageViergeDesc")}</span>
      </button>
    {/if}
    {#each $presetsStore as preset (preset.id)}
      <button
        class="preset-card"
        onclick={() => choisirPreset(preset.id, preset.nom)}
        disabled={occupe}
      >
        <svg class="preset-ico" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
          {@html iconePour(preset.id)}
        </svg>
        <span class="preset-nom">{preset.nom}</span>
        <span class="preset-desc">{preset.description}</span>
      </button>
    {/each}
    {#if $presetsStore.length === 0}
      <p class="vide">{$t("presetsModal.aucun")}</p>
    {/if}
  </div>

  {#if mode === "onboarding"}
    <div class="onboarding-footer">
      <button class="modal-action" onclick={fermerOnboarding} disabled={occupe}>
        {$t("presetsModal.partirVierge")}
      </button>
    </div>
  {/if}
</Modal>

<style>
  .nom-row {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    padding: 0.7rem 1rem 0.4rem;
  }
  .nom-row label {
    font-size: 0.8rem;
    color: var(--gris);
    flex-shrink: 0;
  }
  .nom-row input {
    flex: 1;
    min-width: 0;
    background: var(--fond-controle);
    color: var(--texte);
    border: 1px solid var(--bordure-ui);
    padding: 0.35rem 0.6rem;
    font-size: 0.85rem;
  }
  .nom-row input:focus {
    outline: none;
    border-color: var(--accent-ui);
    box-shadow: var(--glow-ui);
  }
  .grille {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(215px, 1fr));
    gap: 0.7rem;
    padding: 0.8rem 1rem 1rem;
    overflow-y: auto;
  }
  /* Card preset : recette « verre sombre » (btn-surface + inset) — la couleur
     d'identité vit en bordure/glow au survol (pattern .modal-action). */
  .preset-card {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 0.3rem;
    text-align: left;
    padding: 0.9rem;
    background: var(--btn-surface);
    box-shadow: var(--btn-inset);
    color: var(--texte);
    border: 1px solid var(--bordure-ui);
    border-radius: var(--rayon);
    cursor: pointer;
    transition: background 0.15s ease, box-shadow 0.15s ease, border-color 0.15s ease;
  }
  .preset-card:hover:not(:disabled) {
    background: var(--btn-surface-hover);
    border-color: var(--accent-ui);
    box-shadow: var(--btn-inset-hover), var(--glow-ui);
  }
  .preset-card:active:not(:disabled) {
    background: var(--btn-surface-active);
    box-shadow: var(--btn-inset-active);
  }
  .preset-card:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .preset-ico {
    width: 26px;
    height: 26px;
    color: var(--accent-ui);
    margin-bottom: 0.2rem;
    flex-shrink: 0;
  }
  .preset-nom {
    font-weight: 600;
    font-size: 0.95rem;
  }
  .preset-desc {
    font-size: 0.78rem;
    color: var(--gris);
    line-height: 1.35;
  }
  .onboarding-footer {
    display: flex;
    justify-content: flex-end;
    gap: 0.4rem;
    padding: 0.7rem 1rem;
    border-top: 1px solid var(--bordure-ui);
  }
  .vide {
    padding: 1rem;
    color: var(--gris);
    font-size: 0.85rem;
    margin: 0;
    grid-column: 1 / -1;
  }
</style>
