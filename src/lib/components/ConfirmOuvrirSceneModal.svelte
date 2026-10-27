<script lang="ts">
  // Modale de confirmation d'ouverture d'une scène masquée (pastille au
  // titre orange de la barre « Vos scènes »). Ouverte par le clic gauche sur
  // la pastille (SceneBar → confirmOuvrirSceneModalOpen = id de la scène —
  // pattern dupliquerSceneModalOpen : cible portée par la valeur, montage
  // par App.svelte seulement si non-null).
  // Le hint rappelle le titre de la scène avant sa mise à l'antenne.
  // Raccourcis : Enter = Oui, Escape = Non (géré par le wrapper Modal).
  import { confirmOuvrirSceneModalOpen } from "../stores/ui";
  import { scenesIndexStore, openScene } from "../stores/scenes";
  import Modal from "./Modal.svelte";
  import { t } from "../stores/langue";

  let sourceId = $derived($confirmOuvrirSceneModalOpen);
  let source = $derived(
    sourceId ? $scenesIndexStore.find((s) => s.id === sourceId) ?? null : null
  );

  async function onConfirm() {
    const id = sourceId;
    confirmOuvrirSceneModalOpen.set(null);
    if (id) await openScene(id);
  }

  function onCancel() {
    confirmOuvrirSceneModalOpen.set(null);
  }

  // Enter = confirmer (Escape est géré par le wrapper Modal → onCancel).
  function onKeydown(e: KeyboardEvent) {
    if (!sourceId) return;
    if (e.key === "Enter") {
      e.preventDefault();
      e.stopPropagation();
      void onConfirm();
    }
  }
</script>

<svelte:window onkeydown={onKeydown} />

{#if sourceId}
  <Modal
    title={$t("confirmOuvrirScene.titre")}
    onClose={onCancel}
    maxWidth="24rem"
    hint={source
      ? $t("confirmOuvrirScene.hintNom", { nom: source.nom })
      : $t("confirmOuvrirScene.hintGenerique")}
    crochet="var(--surbrillance-orange)"
  >
    <div class="confirm-body">
      <div class="modal-actions confirm-actions">
        <button class="action primary" onclick={() => void onConfirm()}>{$t("commun.oui")}</button>
        <button class="action" onclick={onCancel}>{$t("commun.non")}</button>
      </div>
    </div>
  </Modal>
{/if}

<style>
  .confirm-body {
    padding: 1rem;
  }
  .confirm-actions {
    display: grid;
    grid-template-columns: 1fr 1fr;
    justify-content: stretch;
    padding-top: 0;
    border-top: none;
  }
</style>
