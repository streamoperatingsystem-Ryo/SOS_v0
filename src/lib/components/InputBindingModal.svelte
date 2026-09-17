<script lang="ts">
  // Modale de binding des touches manette (input-viewer mode manette).
  // Permet de remapper chaque bouton affiché vers un index physique différent
  // + inverser les axes X/Y du D-pad (bug 8BitDo etc.).
  // Détection des presses via navigator.getGamepads() (WebView2 = XInput) +
  // input manuel de l'index (fallback si gamepad non visible par le navigateur).
  import { setInputViewerConfig } from "../stores/scene";
  import type { InputMapping } from "../tauri";
  import { untrack } from "svelte";
  import Modal from "./Modal.svelte";

  let {
    widgetId,
    mapping,
    onFermer,
  }: {
    widgetId: string;
    mapping: InputMapping;
    onFermer: () => void;
  } = $props();

  // Labels des 16 boutons affichés (index → nom générique multi-skin).
  const BTN_LABELS = [
    "A / ✕", "B / ○", "X / □", "Y / △",
    "LB / L1", "RB / R1", "LT / L2", "RT / R2",
    "Select / Share", "Start / Options",
    "L3", "R3",
    "D-Pad Haut", "D-Pad Bas", "D-Pad Gauche", "D-Pad Droite",
  ];

  // Copie locale de travail (annulation possible). untrack : on veut la
  // valeur initiale seulement (pas de re-sync si la prop change).
  const defaultBtns = [0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15];
  const init = untrack(() => ({
    btns: mapping.buttons ?? defaultBtns,
    invY: mapping.dpadInvertY ?? false,
    invX: mapping.dpadInvertX ?? false,
  }));
  let buttons = $state<number[]>([...init.btns]);
  let dpadInvertY = $state(init.invY);
  let dpadInvertX = $state(init.invX);

  // Binding interactif : index du bouton en attente de press, ou -1.
  let bindingIdx = $state(-1);
  let rafId = 0;
  // Détection gamepad : indique si navigator.getGamepads() voit une manette.
  let gamepadDetecte = $state(false);
  let pollCount = $state(0);

  /// Démarre le binding pour un bouton affiché. Poll navigator.getGamepads()
  /// jusqu'à ce qu'un bouton soit pressé → enregistre l'index physique.
  function startBinding(displayIdx: number) {
    bindingIdx = displayIdx;
    pollCount = 0;
    cancelAnimationFrame(rafId);

    function poll() {
      const pads = navigator.getGamepads();
      let anyPad = false;
      for (const pad of pads) {
        if (!pad) continue;
        anyPad = true;
        for (let i = 0; i < pad.buttons.length; i++) {
          if (pad.buttons[i].pressed) {
            buttons[displayIdx] = i;
            bindingIdx = -1;
            gamepadDetecte = true;
            return;
          }
        }
      }
      if (anyPad) gamepadDetecte = true;
      pollCount++;
      // Timeout après ~10s sans détection.
      if (pollCount > 600) {
        bindingIdx = -1;
        return;
      }
      rafId = requestAnimationFrame(poll);
    }
    poll();
  }

  function cancelBinding() {
    bindingIdx = -1;
    cancelAnimationFrame(rafId);
  }

  /// Remet un bouton à sa valeur par défaut (identité).
  function resetButton(displayIdx: number) {
    buttons[displayIdx] = displayIdx;
  }

  /// Remet tout par défaut.
  function resetAll() {
    buttons = [0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15];
    dpadInvertY = false;
    dpadInvertX = false;
  }

  /// Modifie l'index d'un bouton via l'input manuel (fallback).
  function setButtonManual(displayIdx: number, value: string) {
    const n = parseInt(value, 10);
    if (!isNaN(n) && n >= 0 && n <= 19) {
      buttons[displayIdx] = n;
    }
  }

  async function onSave() {
    cancelBinding();
    await setInputViewerConfig(widgetId, {
      inputMapping: { buttons, dpadInvertY, dpadInvertX },
    });
    onFermer();
  }

  function onCancel() {
    cancelBinding();
    onFermer();
  }
</script>

<Modal
  title="Binding des touches manette"
  onClose={onCancel}
  maxWidth="34rem"
  hint="Cliquez « Binder » puis appuyez sur la touche physique correspondante. Vous pouvez aussi saisir l'index manuellement (0-19)."
>
  <div class="modal-scroll">
    <div class="btn-list">
      {#each BTN_LABELS as label, i}
        <div class="btn-row" class:binding={bindingIdx === i}>
          <span class="btn-label">{label}</span>
          <input
            type="number"
            min="0"
            max="19"
            value={buttons[i]}
            onchange={(e) => setButtonManual(i, (e.target as HTMLInputElement).value)}
            class="btn-input"
          />
          {#if bindingIdx === i}
            <button class="bind-btn binding" onclick={cancelBinding}>En attente…</button>
          {:else}
            <button class="bind-btn" onclick={() => startBinding(i)}>Binder</button>
          {/if}
          {#if buttons[i] !== i}
            <button class="reset-btn" onclick={() => resetButton(i)} title="Réinitialiser">↺</button>
          {/if}
        </div>
      {/each}
    </div>

    {#if bindingIdx >= 0 && !gamepadDetecte && pollCount > 30}
      <p class="warn">
        Aucune manette détectée par le navigateur. Saisissez l'index manuellement
        (gilrs : 0=A, 1=B, 2=X, 3=Y, 4=LB, 5=RB, 6=LT, 7=RT, 8=Select,
        9=Start, 10=L3, 11=R3, 12=DPad↑, 13=DPad↓, 14=DPad←, 15=DPad→).
      </p>
    {/if}

    <div class="dpad-section">
      <h3>D-Pad (axes)</h3>
      <p class="note">Pour les manettes qui reportent le D-pad sur les axes (8BitDo rétro).</p>
      <label class="toggle-row">
        <input type="checkbox" checked={dpadInvertY} onchange={(e) => (dpadInvertY = (e.target as HTMLInputElement).checked)} />
        <span>Inverser axe Y (Haut ↔ Bas)</span>
      </label>
      <label class="toggle-row">
        <input type="checkbox" checked={dpadInvertX} onchange={(e) => (dpadInvertX = (e.target as HTMLInputElement).checked)} />
        <span>Inverser axe X (Gauche ↔ Droite)</span>
      </label>
    </div>

    <div class="modal-actions">
      <button class="modal-action" onclick={resetAll}>Réinitialiser tout</button>
      <button class="modal-action" onclick={onCancel}>Annuler</button>
      <button class="modal-action primary" onclick={onSave}>Enregistrer</button>
    </div>
  </div>
</Modal>

<style>
  h3 {
    font-size: 0.85rem;
    margin: 0;
    opacity: 0.85;
  }
  .note {
    font-size: 0.78rem;
    opacity: 0.6;
    line-height: 1.3;
  }
  .warn {
    font-size: 0.75rem;
    color: var(--message-user-action-color);
    line-height: 1.3;
    padding: 0.4rem 0.5rem;
    border: 1px solid var(--bordure);
    border-radius: var(--rayon-petit);
  }
  .btn-list {
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
  }
  .btn-row {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.25rem 0.4rem;
    border-radius: var(--rayon-petit);
    transition: background 80ms;
  }
  .btn-row.binding {
    background: color-mix(in srgb, var(--message-user-action-color) 15%, transparent);
  }
  .btn-label {
    flex: 1;
    font-size: 0.82rem;
  }
  .btn-input {
    width: 3rem;
    background: var(--fond-controle);
    color: var(--texte);
    border: 1px solid var(--bordure);
    border-radius: var(--rayon-petit);
    padding: 0.15rem 0.3rem;
    font: inherit;
    font-size: 0.78rem;
    text-align: center;
    font-variant-numeric: tabular-nums;
  }
  .btn-input:focus {
    outline: none;
    border-color: var(--accent-violet);
  }
  .bind-btn {
    background: var(--btn-surface);
    box-shadow: var(--btn-inset);
    color: var(--texte);
    border: 1px solid var(--bordure);
    padding: 0.2rem 0.5rem;
    font: inherit;
    font-size: 0.78rem;
    cursor: pointer;
    border-radius: var(--rayon-petit);
    transition: background 0.15s ease, box-shadow 0.15s ease, border-color 0.15s ease;
  }
  .bind-btn:hover {
    background: var(--btn-surface-hover);
    box-shadow: var(--btn-inset-hover);
    border-color: var(--accent-violet);
  }
  .bind-btn.binding {
    background: var(--message-user-action-color);
    color: var(--fond);
    border-color: var(--message-user-action-color);
    animation: pulse 1s ease-in-out infinite;
  }
  @keyframes pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.6; }
  }
  .reset-btn {
    background: none;
    border: none;
    color: var(--gris);
    cursor: pointer;
    font-size: 0.9rem;
    padding: 0 0.2rem;
  }
  .reset-btn:hover {
    color: var(--texte);
  }
  .dpad-section {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
    padding-top: 0.6rem;
    border-top: 1px solid var(--bordure);
  }
  .toggle-row {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    font-size: 0.82rem;
    cursor: pointer;
  }
  .toggle-row input {
    cursor: pointer;
  }
</style>
