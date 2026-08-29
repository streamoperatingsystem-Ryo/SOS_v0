<script lang="ts">
  // Modale de confirmation suppression widget (clic bouton Toolbar OU touche Suppr).
  // État partagé via le store confirmDeleteWidget (ui.ts) — ouvert depuis
  // Toolbar.svelte (bouton) et App.svelte (touche Suppr).
  // Raccourcis : Enter = Oui, Escape = Non.
  import { confirmDeleteWidget } from "../stores/ui";
  import { deleteWidget, selectedIdStore as selId } from "../stores/scene";
  import { get } from "svelte/store";

  let open = $derived($confirmDeleteWidget);

  async function onConfirm() {
    const id = get(selId);
    confirmDeleteWidget.set(false);
    if (id) await deleteWidget(id);
  }

  function onCancel() {
    confirmDeleteWidget.set(false);
  }

  function onKeydown(e: KeyboardEvent) {
    if (!open) return;
    if (e.key === "Enter") {
      e.preventDefault();
      e.stopPropagation();
      onConfirm();
    } else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      onCancel();
    }
  }
</script>

<svelte:window onkeydown={onKeydown} />

{#if open}
  <div class="overlay" onclick={onCancel} role="presentation">
    <div
      class="modal"
      onclick={(e) => e.stopPropagation()}
      role="dialog"
      aria-modal="true"
      tabindex="-1"
    >
      <h2>Supprimer ce widget ?</h2>
      <p class="hint">Cette action est irréversible.</p>
      <div class="buttons">
        <button class="action danger" onclick={onConfirm}>Oui</button>
        <button class="action" onclick={onCancel}>Non</button>
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
    padding: 1.2rem;
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
    min-width: 18rem;
    max-width: 24rem;
  }
  h2 {
    font-size: 1rem;
    margin: 0;
  }
  .hint {
    font-size: 0.8rem;
    opacity: 0.7;
    margin: 0;
  }
  .buttons {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 0.4rem;
    margin-top: 0.2rem;
  }
  .action {
    background: var(--fond);
    color: var(--texte);
    border: 1px solid var(--texte);
    padding: 0.4rem 0.6rem;
    font: inherit;
    cursor: pointer;
    text-align: center;
  }
  .action:hover {
    background: var(--texte);
    color: var(--fond);
  }
  .action.danger:hover {
    background: #c0392b;
    color: #fff;
    border-color: #c0392b;
  }
</style>
