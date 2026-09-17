<script lang="ts">
  // Modale de confirmation suppression widget (clic bouton Toolbar OU touche Suppr).
  // État partagé via le store confirmDeleteWidget (ui.ts) — ouvert depuis
  // Toolbar.svelte (bouton) et App.svelte (touche Suppr).
  // Raccourcis : Enter = Oui, Escape = Non (géré par le wrapper Modal).
  import { confirmDeleteWidget } from "../stores/ui";
  import { deleteWidget, selectedIdStore as selId } from "../stores/scene";
  import { get } from "svelte/store";
  import Modal from "./Modal.svelte";

  let open = $derived($confirmDeleteWidget);

  async function onConfirm() {
    const id = get(selId);
    confirmDeleteWidget.set(false);
    if (id) await deleteWidget(id);
  }

  function onCancel() {
    confirmDeleteWidget.set(false);
  }

  // Enter = confirmer (Escape est géré par le wrapper Modal → onCancel).
  function onKeydown(e: KeyboardEvent) {
    if (!open) return;
    if (e.key === "Enter") {
      e.preventDefault();
      e.stopPropagation();
      onConfirm();
    }
  }
</script>

<svelte:window onkeydown={onKeydown} />

{#if open}
  <Modal title="Supprimer ce widget ?" onClose={onCancel} maxWidth="24rem" hint="Cette action est irréversible.">
    <div class="confirm-body">
      <div class="modal-actions confirm-actions">
        <button class="modal-action danger" onclick={onConfirm}>Oui</button>
        <button class="modal-action" onclick={onCancel}>Non</button>
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
