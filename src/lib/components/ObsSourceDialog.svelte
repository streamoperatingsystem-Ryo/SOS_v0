<script lang="ts">
  import { tauri } from "../tauri";
  import { createObsTrouFromPc } from "../stores/scene";

  let { widgetId, onClose }: { widgetId: string; onClose: () => void } = $props();

  // 2 étapes : "type" (3 boutons) → "liste" (cibles PC vivantes) → "working".
  type Mode = "type" | "liste" | "working";
  let mode = $state<Mode>("type");
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
    mode = "liste";
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
      console.log(`[OBS trou] ${k} → ${liste.length} cible(s) PC`);
    } catch (e) {
      error = String(e?.message ?? e);
    } finally {
      loading = false;
    }
  }

  // Clic item → créer la source OBS (CreateInput ou SetInputSettings si existe).
  async function onPick(item: CamItem | WinItem) {
    if (!kind) return;
    // target : caméra = nom device, fenêtre/jeu = obs_value ("title:class:exe").
    const target = "obs_value" in item ? item.obs_value : item.nom;
    console.log("[OBS trou] invoke", kind, item, "target=", target, "widgetId=", widgetId);
    mode = "working";
    working = true;
    error = null;
    try {
      await createObsTrouFromPc(widgetId, kind, target);
      console.log("[OBS trou] invoke OK, fermeture dialog");
      onClose();
    } catch (e) {
      const msg = String(e?.message ?? e);
      console.error("[OBS trou] create_trou_from_pc échec:", msg, e);
      error = msg;
      mode = "liste";
    } finally {
      working = false;
    }
  }

  function onBack() {
    mode = "type";
    kind = null;
    liste = [];
    error = null;
  }
</script>

<div class="overlay" onclick={onClose} role="presentation">
  <div class="dialog" onclick={(e) => e.stopPropagation()} role="dialog" tabindex="-1">
    <h2>Source OBS sous SOS</h2>

    {#if mode === "type"}
      <p class="hint">Choisir le type de source :</p>
      <div class="type-buttons">
        <button class="action" onclick={() => pickType("camera")}>Caméra</button>
        <button class="action" onclick={() => pickType("window")}>Fenêtre</button>
        <button class="action" onclick={() => pickType("game")}>Jeu</button>
      </div>

    {:else if mode === "liste" || mode === "working"}
      <div class="list-header">
        <button class="action small" onclick={onBack} disabled={working}>← Retour</button>
        <span class="list-title">{kind}</span>
      </div>

      {#if loading}
        <p class="hint">Chargement…</p>
      {:else if liste.length === 0}
        <p class="hint warn">Aucune cible PC de ce type.</p>
      {:else}
        <div class="list">
          {#each liste as item (item.nom)}
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
        <p class="hint warn">{error}</p>
      {/if}
    {/if}

    <button class="cancel" onclick={onClose}>Annuler</button>
  </div>
</div>

<style>
  .overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.6);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
  }
  .dialog {
    background: var(--fond);
    color: var(--texte);
    border: 1px solid var(--texte);
    padding: 1rem 1.2rem;
    min-width: 320px;
    max-width: 460px;
    max-height: 80vh;
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
  }
  h2 {
    font-size: 1rem;
    margin: 0;
  }
  .hint {
    font-size: 0.85rem;
    opacity: 0.8;
    margin: 0;
  }
  .hint.warn {
    opacity: 1;
    color: #e0a060;
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
    background: var(--fond);
    color: var(--texte);
    border: 1px solid var(--texte);
    padding: 0.4rem 0.6rem;
    font: inherit;
    font-size: 0.85rem;
    cursor: pointer;
    text-align: left;
    word-break: break-word;
  }
  .row:hover:not(:disabled) {
    background: var(--texte);
    color: var(--fond);
  }
  .row:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
  .exe {
    opacity: 0.6;
    font-size: 0.8rem;
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
  .action:hover:not(:disabled) {
    background: var(--texte);
    color: var(--fond);
  }
  .action:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
  .action.small {
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
