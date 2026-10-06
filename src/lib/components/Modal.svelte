<script lang="ts">
  // Wrapper de modale — source unique de la structure externe (overlay +
  // container + header titre/✕ + hint optionnel + body scrollable). Toutes
  // les modales de l'app passent par ce composant pour garantir le même
  // style et la même organisation (cf. AGENTS.md → Modales).
  //
  // Slot `header-extra` : contenu optionnel dans le header (ex: onglets de
  // CadresModal). Slot par défaut (`children`) : corps de la modale.
  import type { Snippet } from "svelte";
  import { t } from "../stores/langue";

  // Prop `crochet` : couleur du liseré vertical pleine hauteur sur le bord
  // gauche de la modale — même motif que le crochet des sections du
  // dashboard (box-shadow inset 3px). Chaque modale passe la couleur du
  // bouton/section qui la lance (rôles --crochet-* / accents) ; défaut
  // --accent-ui pour les modales sans couleur dédiée.
  let {
    title,
    onClose,
    maxWidth = "900px",
    hint = "",
    headerExtra = null,
    crochet = "var(--accent-ui)",
    children,
  }: {
    title: string;
    onClose: () => void;
    maxWidth?: string;
    hint?: string;
    headerExtra?: Snippet | null;
    crochet?: string;
    children: Snippet;
  } = $props();

  let downSurOverlay = false;

  function onOverlayPointerDown(e: PointerEvent) {
    downSurOverlay = e.target === e.currentTarget;
  }

  function onOverlayPointerUp(e: PointerEvent) {
    if (e.button === 0 && downSurOverlay && e.target === e.currentTarget) {
      onClose();
    }
    downSurOverlay = false;
  }

  function onOverlayPointerCancel() {
    downSurOverlay = false;
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      onClose();
    }
  }
</script>

<svelte:window onkeydown={onKeydown} />

<div
  class="modal-overlay"
  onpointerdown={onOverlayPointerDown}
  onpointerup={onOverlayPointerUp}
  onpointercancel={onOverlayPointerCancel}
  role="presentation"
>
  <div
    class="modal-shell"
    style="max-width: {maxWidth}; --crochet-modal: {crochet}"
    onclick={(e) => e.stopPropagation()}
    onkeydown={(e) => e.stopPropagation()}
    role="dialog"
    aria-modal="true"
    aria-label={title}
    tabindex="-1"
  >
    <div class="modal-header">
      <h2>{title}</h2>
      {#if headerExtra}
        <div class="modal-header-extra">
          {@render headerExtra()}
        </div>
      {/if}
      <button class="modal-close" onclick={onClose} aria-label={$t("commun.fermer")}>✕</button>
    </div>
    {#if hint}
      <p class="modal-hint">{hint}</p>
    {/if}
    <div class="modal-body">
      {@render children()}
    </div>
  </div>
</div>

<style>
  .modal-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.7);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 10000;
  }
  .modal-shell {
    background: var(--fond-panneau);
    color: var(--texte);
    border: 1px solid var(--bordure-ui-active);
    border-radius: var(--rayon-grand);
    box-shadow:
      0 0 40px color-mix(in srgb, var(--accent-ui) 15%, transparent),
      /* Crochet pleine hauteur : liseré 3px sur le bord gauche, couleur du
         bouton lanceur (--crochet-modal, prop crochet). L'inset suit le
         border-radius — extrémités arrondies comme les sections. */
      inset 3px 0 0 var(--crochet-modal);
    display: flex;
    flex-direction: column;
    width: 90vw;
    max-height: 85vh;
    overflow: hidden;
    /* Boutons internes = fill glace neutre commun ; le violet néon vit en
       bordure/glow/hover uniquement (IDEA02). */
  }
  .modal-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
    padding: 0.75rem 1rem;
    border-bottom: 1px solid var(--bordure-ui);
  }
  h2 {
    font-size: 1rem;
    margin: 0;
    font-weight: 600;
  }
  .modal-header-extra {
    display: flex;
    align-items: center;
    gap: 0.3rem;
    flex: 1;
    justify-content: center;
  }
  .modal-close {
    background: var(--btn-surface);
    box-shadow: var(--btn-inset);
    color: var(--texte);
    border: 1px solid var(--bordure-ui);
    font-size: 0.85rem;
    cursor: pointer;
    padding: 0.2rem 0.5rem;
    line-height: 1;
    flex-shrink: 0;
    transition: background 0.15s ease, box-shadow 0.15s ease, border-color 0.15s ease, color 0.15s ease;
  }
  .modal-close:hover {
    background: var(--btn-surface-hover);
    box-shadow: var(--btn-inset-hover);
    border-color: var(--message-error-color);
    color: var(--message-error-color);
  }
  .modal-hint {
    font-size: 0.78rem;
    opacity: 0.75;
    padding: 0.45rem 1rem;
    border-bottom: 1px solid var(--bordure-ui);
    margin: 0;
    line-height: 1.3;
  }
  .modal-body {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-height: 0;
    overflow: hidden;
  }
</style>
