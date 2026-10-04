<script lang="ts">
  import { tauri } from "../tauri";
  import { createObsTrouFromPc, lierFondCapture } from "../stores/scene";
  import Modal from "./Modal.svelte";
  import { t } from "../stores/langue";

  // mode "widget" (défaut) : source liée au widget trou sélectionné (widgetId).
  // mode "fond" : capture plein canvas comme FOND de la scène (pas de
  // widgetId — source "SOS-Fond-<sceneId8>" gérée par lierFondCapture). La
  // caméra n'est PAS proposée en mode fond : conflit d'accès exclusif dshow
  // avec un éventuel widget caméra sur le même device.
  let {
    widgetId,
    mode = "widget",
    onClose,
  }: { widgetId?: string; mode?: "widget" | "fond"; onClose: () => void } = $props();

  // 2 étapes : "type" (boutons) → "liste" (cibles PC vivantes) → "working".
  type Etape = "type" | "liste" | "working";
  let etape = $state<Etape>("type");
  let kind = $state<"camera" | "window" | "game" | null>(null);

  // Caméra : nom device (String). Fenêtre/Jeu : { title, exe, obs_value }.
  type CamItem = { nom: string };
  type WinItem = { nom: string; exe: string; obs_value: string };
  let liste = $state<(CamItem | WinItem)[]>([]);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let working = $state(false);

  // Étape 1 → Étape 2 : charge la liste vivante PC (pas GetInputList).
  async function pickType(k: "camera" | "window" | "game") {
    kind = k;
    etape = "liste";
    loading = true;
    error = null;
    liste = [];
    try {
      if (k === "camera") {
        const devices = await tauri.pcEnumerateCameras();
        liste = devices.map((d) => ({ nom: d }));
      } else if (k === "window") {
        const wins = await tauri.pcEnumerateWindows();
        liste = wins.map((w) => ({ nom: w.title, exe: w.exe, obs_value: w.obs_value }));
      } else {
        const games = await tauri.pcEnumerateGames();
        liste = games.map((g) => ({ nom: g.title, exe: g.exe, obs_value: g.obs_value }));
      }
    } catch (e) {
      error = String((e as Error)?.message ?? e);
    } finally {
      loading = false;
    }
  }

  // Clic item → créer la source OBS du widget, ou lier la capture de fond.
  async function onPick(item: CamItem | WinItem) {
    if (!kind) return;
    // target : caméra = nom device, fenêtre/jeu = obs_value ("title:class:exe").
    const target = "obs_value" in item ? item.obs_value : item.nom;
    etape = "working";
    working = true;
    error = null;
    try {
      if (mode === "fond") {
        await lierFondCapture(kind, target);
      } else {
        if (!widgetId) return;
        await createObsTrouFromPc(widgetId, kind, target);
      }
      onClose();
    } catch (e) {
      const msg = String((e as Error)?.message ?? e);
      error = msg;
      etape = "liste";
    } finally {
      working = false;
    }
  }

  function onBack() {
    etape = "type";
    kind = null;
    liste = [];
    error = null;
  }
</script>

<Modal
  title={mode === "fond" ? $t("obsSourceDialog.titreFond") : $t("obsSourceDialog.titreWidget")}
  onClose={onClose}
  maxWidth="460px"
  hint={mode === "fond"
    ? $t("obsSourceDialog.hintFond")
    : $t("obsSourceDialog.hintWidget")}
>
  <div class="dialog-body">
    {#if etape === "type"}
      <p class="note">{$t("obsSourceDialog.choisirType")}</p>
      <div class="type-buttons">
        {#if mode !== "fond"}
          <button class="modal-action" onclick={() => pickType("camera")}>{$t("obsSourceDialog.kind.camera")}</button>
        {/if}
        <button class="modal-action" onclick={() => pickType("window")}>{$t("obsSourceDialog.kind.window")}</button>
        <button class="modal-action" onclick={() => pickType("game")}>{$t("obsSourceDialog.kind.game")}</button>
      </div>

    {:else if etape === "liste" || etape === "working"}
      <div class="list-header">
        <button class="modal-action small" onclick={onBack} disabled={working}>← {$t("obsSourceDialog.retour")}</button>
        <span class="list-title">{$t("obsSourceDialog.kind." + kind)}</span>
      </div>

      {#if loading}
        <p class="note">{$t("commun.chargement")}</p>
      {:else if liste.length === 0}
        <p class="note warn">{$t("obsSourceDialog.aucuneCible")}</p>
      {:else}
        <div class="list">
          {#each liste as item, i (i)}
            <button
              class="row"
              onclick={() => onPick(item)}
              disabled={working}
            >
              {item.nom}
              {#if "exe" in item && item.exe}
                <span class="exe"> [{item.exe}]</span>
              {/if}
            </button>
          {/each}
        </div>
      {/if}

      {#if error}
        <p class="note warn">{error}</p>
      {/if}
    {/if}

    <button class="cancel" onclick={onClose}>{$t("commun.annuler")}</button>
  </div>
</Modal>

<style>
  .dialog-body {
    padding: 1rem 1.2rem;
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
  }
  .note {
    font-size: 0.85rem;
    opacity: 0.8;
    margin: 0;
  }
  .note.warn {
    opacity: 1;
    color: var(--message-user-action-color);
  }
  .type-buttons {
    display: grid;
    grid-template-columns: 1fr 1fr 1fr;
    gap: 0.4rem;
  }
  .list-header {
    display: flex;
    align-items: center;
    gap: 0.6rem;
  }
  .list-title {
    font-size: 0.85rem;
    text-transform: capitalize;
    opacity: 0.8;
  }
  .list {
    overflow-y: auto;
    max-height: 320px;
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
  }
  .row {
    background: var(--btn-surface);
    box-shadow: var(--btn-inset);
    color: var(--texte);
    border: 1px solid var(--bordure-ui);
    padding: 0.4rem 0.6rem;
    font: inherit;
    font-size: 0.85rem;
    cursor: pointer;
    text-align: left;
    word-break: break-word;
    transition: background 0.15s ease, box-shadow 0.15s ease, border-color 0.15s ease;
  }
  .row:hover:not(:disabled) {
    background: var(--btn-surface-hover);
    box-shadow: var(--btn-inset-hover);
    border-color: var(--accent-ui);
  }
  .row:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
  .exe {
    opacity: 0.6;
    font-size: 0.8rem;
  }
  .modal-action.small {
    padding: 0.25rem 0.5rem;
    font-size: 0.8rem;
  }
  .cancel {
    background: transparent;
    color: var(--texte);
    border: none;
    padding: 0.3rem;
    font: inherit;
    font-size: 0.85rem;
    cursor: pointer;
    opacity: 0.6;
  }
  .cancel:hover {
    opacity: 1;
  }
</style>
