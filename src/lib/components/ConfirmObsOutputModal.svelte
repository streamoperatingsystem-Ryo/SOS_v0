<script lang="ts">
  // Modale de confirmation des sorties OBS (stream / record) — démarrer ET
  // arrêter demandent confirmation (dans OBS un clic suffit ; dans SOS non).
  // Posée par les 3 lanceurs : Toolbar, topbar App.svelte et barre de
  // raccourcis (dispatchRaccourcis — la modale s'ouvre dans le dashboard).
  // Cible portée par la valeur de confirmObsOutputOpen (ui.ts) — même
  // pattern que confirmOuvrirSceneModalOpen. Oui → toggle*Obs (one-shot OBS).
  // Raccourcis : Enter = Oui, Escape = Non (géré par le wrapper Modal).
  import { confirmObsOutputOpen } from "../stores/ui";
  import {
    obsStreaming,
    obsRecording,
    setStreamObs,
    setRecordObs,
    rafraichirOutputs,
  } from "../stores/obs";
  import Modal from "./Modal.svelte";
  import { t } from "../stores/langue";

  let kind = $derived($confirmObsOutputOpen);
  // État actuel de la sortie ciblée → le set visé est un arrêt si actif.
  let actif = $derived(
    kind === "stream" ? $obsStreaming : kind === "record" ? $obsRecording : false
  );
  // Suffixe des clés i18n : stream/record × start/stop → 4 paires titre/hint.
  let suffixe = $derived(
    `${kind === "stream" ? "Stream" : "Record"}${actif ? "Stop" : "Start"}`
  );

  // Resync à l'ouverture : la modale affiche l'état RÉEL relu dans OBS à
  // l'instant T, pas le cache (l'utilisateur a pu agir directement dans
  // OBS — bouton OBS, stream tombé…). « Oui » grisé pendant la lecture :
  // confirmer sur un libellé périmé exécuterait l'inverse de l'intention.
  let verif = $state(false);
  $effect(() => {
    if (!kind) return;
    verif = true;
    void rafraichirOutputs().finally(() => {
      verif = false;
    });
  });

  async function onConfirm() {
    // Enter contourne le disabled du bouton → la garde vit ici aussi :
    // jamais de confirmation sur un libellé pas encore resynchronisé.
    if (verif) return;
    const k = kind;
    // Intention = l'inverse de l'état affiché au clic (relus à l'ouverture).
    const cible = !actif;
    confirmObsOutputOpen.set(null);
    if (k === "stream") await setStreamObs(cible);
    else if (k === "record") await setRecordObs(cible);
  }

  function onCancel() {
    confirmObsOutputOpen.set(null);
  }

  // Enter = confirmer (Escape est géré par le wrapper Modal → onCancel).
  function onKeydown(e: KeyboardEvent) {
    if (!kind) return;
    if (e.key === "Enter") {
      e.preventDefault();
      e.stopPropagation();
      void onConfirm();
    }
  }
</script>

<svelte:window onkeydown={onKeydown} />

{#if kind}
  <Modal
    title={$t(`confirmObsOutput.titre${suffixe}`)}
    onClose={onCancel}
    maxWidth="24rem"
    hint={$t(`confirmObsOutput.hint${suffixe}`)}
    crochet={actif ? "var(--message-error-color)" : "var(--accent-ui)"}
  >
    <div class="confirm-body">
      <div class="modal-actions confirm-actions">
        <button
          class="action"
          class:primary={!actif}
          class:danger={actif}
          disabled={verif}
          onclick={() => void onConfirm()}
        >{$t("commun.oui")}</button>
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
