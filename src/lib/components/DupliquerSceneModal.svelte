<script lang="ts">
  // DupliquerSceneModal — nommage de la copie d'une scène.
  // Ouverte par le menu contextuel d'une pastille (SceneBar → « Dupliquer ») :
  // la valeur du store dupliquerSceneModalOpen = id de la scène source
  // (pattern titreModalOpen — cible portée par la valeur, montage par
  // App.svelte seulement si non-null). La copie reprend TOUT le contenu
  // (widgets, fond, cadre — ids widgets régénérés côté Rust, obsSource
  // conservé : source OBS partagée), est ajoutée en fin de liste et
  // devient la scène courante.
  import { dupliquerSceneModalOpen } from "../stores/ui";
  import { scenesIndexStore, duplicateScene } from "../stores/scenes";
  import Modal from "./Modal.svelte";

  let sourceId = $derived($dupliquerSceneModalOpen);
  let source = $derived(
    sourceId ? $scenesIndexStore.find((s) => s.id === sourceId) ?? null : null
  );

  // Suggestion : « <nom source> 2 », incrémentée tant que le nom est pris.
  let suggestion = $derived.by(() => {
    const base = source?.nom ?? "Scène";
    const pris = new Set($scenesIndexStore.map((s) => s.nom.trim()));
    let n = 2;
    while (pris.has(`${base} ${n}`)) n += 1;
    return `${base} ${n}`;
  });

  // Valeur saisie — pré-remplie à la suggestion à l'ouverture, focus +
  // sélection pour retaper directement. Application en vol = occupe.
  let nomSaisi = $state("");
  let occupe = $state(false);
  let inputEl: HTMLInputElement | undefined = $state(undefined);

  $effect(() => {
    if (sourceId) {
      nomSaisi = suggestion;
      queueMicrotask(() => {
        inputEl?.focus();
        inputEl?.select();
      });
    }
  });

  function fermer() {
    dupliquerSceneModalOpen.set(null);
  }

  async function onValider() {
    if (!sourceId || occupe) return;
    occupe = true;
    try {
      // Vide → suggestion (comme PresetsModal : nomSaisi.trim() || défaut).
      await duplicateScene(sourceId, nomSaisi.trim() || suggestion);
      dupliquerSceneModalOpen.set(null);
    } finally {
      occupe = false;
    }
  }
</script>

{#if sourceId}
  <Modal
    title="Dupliquer la scène"
    onClose={fermer}
    maxWidth="420px"
    hint={source
      ? `Copie de « ${source.nom} » — reprend tout le contenu (widgets, fond, cadre) et devient la scène courante.`
      : "La copie reprend tout le contenu de la scène et devient la scène courante."}
  >
    <div class="dupliquer-corps">
      <div class="nom-row">
        <label for="dupliquer-nom">Nom</label>
        <input
          id="dupliquer-nom"
          bind:this={inputEl}
          type="text"
          bind:value={nomSaisi}
          maxlength="40"
          spellcheck="false"
          disabled={occupe}
          onkeydown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              void onValider();
            }
          }}
        />
      </div>
      <div class="modal-actions">
        <button class="modal-action" onclick={fermer} disabled={occupe}>
          Annuler
        </button>
        <button class="modal-action primary" onclick={() => void onValider()} disabled={occupe}>
          Dupliquer
        </button>
      </div>
    </div>
  </Modal>
{/if}

<style>
  .dupliquer-corps {
    flex: 1;
    display: flex;
    flex-direction: column;
    padding: 0.8rem 1rem 1rem;
    gap: 0.8rem;
  }
  .nom-row {
    display: flex;
    align-items: center;
    gap: 0.6rem;
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
</style>
