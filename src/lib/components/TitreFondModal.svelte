<script lang="ts">
  // TitreFondModal — modale d'édition du titre du fond de l'application.
  // Ouverte par le bouton "Titre du fond" dans la Toolbar (section Arrière-plan).
  // Champs : texte, position (haut/bas intérieur), police Google Font, taille px.
  // Le titre utilise un gradient de texte reprenant les couleurs du cadre de
  // l'application (cadreApp) actif (fallback blanc→gris si aucun cadre actif).
  import { titreFondModalOpen } from "../stores/ui";
  import { sceneStore, setBgTitre } from "../stores/scene";
  import { gradientCadre } from "../cadres/registre";
  import { POLICES_TITRE, POLICE_TITRE_DEFAUT, TAILLE_TITRE_DEFAUT, ESPACEMENT_TITRE_DEFAUT } from "../fonts";
  import Modal from "./Modal.svelte";

  // Valeurs locales éditables (pas de commit en live — seulement au Valider).
  let texte = $state("");
  let position = $state<"haut" | "bas">("haut");
  let police = $state(POLICE_TITRE_DEFAUT);
  let taille = $state(TAILLE_TITRE_DEFAUT);
  let gras = $state(false);
  let italique = $state(false);
  let souligne = $state(false);
  let espacement = $state(ESPACEMENT_TITRE_DEFAUT);

  // Gradient du cadre de l'application pour l'aperçu.
  let cadreApp = $derived($sceneStore.cadreApp ?? null);
  let grad = $derived(gradientCadre(cadreApp));

  // Initialiser les valeurs locales quand la modale s'ouvre.
  $effect(() => {
    if ($titreFondModalOpen) {
      texte = $sceneStore.bgTitre ?? "";
      position = ($sceneStore.bgTitrePosition as "haut" | "bas") ?? "haut";
      police = $sceneStore.bgTitrePolice ?? POLICE_TITRE_DEFAUT;
      taille = $sceneStore.bgTitreTaille ?? TAILLE_TITRE_DEFAUT;
      gras = $sceneStore.bgTitreGras ?? false;
      italique = $sceneStore.bgTitreItalique ?? false;
      souligne = $sceneStore.bgTitreSouligne ?? false;
      espacement = $sceneStore.bgTitreEspacement ?? ESPACEMENT_TITRE_DEFAUT;
    }
  });

  function onValider() {
    const t = texte.trim();
    if (t.length === 0) {
      // Texte vide = supprimer le titre.
      void setBgTitre({
        bgTitre: undefined,
        bgTitrePosition: undefined,
        bgTitrePolice: undefined,
        bgTitreTaille: undefined,
        bgTitreGras: undefined,
        bgTitreItalique: undefined,
        bgTitreSouligne: undefined,
        bgTitreEspacement: undefined,
      });
    } else {
      void setBgTitre({
        bgTitre: t,
        bgTitrePosition: position,
        bgTitrePolice: police,
        bgTitreTaille: Math.max(12, Math.min(72, Math.round(taille))),
        bgTitreGras: gras,
        bgTitreItalique: italique,
        bgTitreSouligne: souligne,
        bgTitreEspacement: Math.max(-2, Math.min(20, Math.round(espacement))),
      });
    }
    titreFondModalOpen.set(false);
  }

  function onSupprimer() {
    void setBgTitre({
      bgTitre: undefined,
      bgTitrePosition: undefined,
      bgTitrePolice: undefined,
      bgTitreTaille: undefined,
      bgTitreGras: undefined,
      bgTitreItalique: undefined,
      bgTitreSouligne: undefined,
      bgTitreEspacement: undefined,
    });
    titreFondModalOpen.set(false);
  }

  function onAnnuler() {
    titreFondModalOpen.set(false);
  }
</script>

{#if $titreFondModalOpen}
  <Modal
    title="Titre du fond de l'application"
    onClose={onAnnuler}
    maxWidth="560px"
    hint="Le titre s'affiche à l'intérieur du fond, en haut ou en bas. Sa couleur reprend automatiquement le dégradé du cadre de l'application."
  >
    <div class="titre-modal-body">
      <!-- Texte -->
      <div class="modal-bloc">
        <div class="modal-bloc-titre">Texte du titre</div>
        <input
          class="titre-input"
          type="text"
          placeholder="Tapez votre titre ici…"
          value={texte}
          maxlength="60"
          oninput={(e) => (texte = (e.target as HTMLInputElement).value)}
        />
      </div>

      <!-- Position -->
      <div class="modal-bloc">
        <div class="modal-bloc-titre">Position</div>
        <div class="position-boutons">
          <button
            class="modal-action {position === 'haut' ? 'primary' : ''}"
            onclick={() => (position = "haut")}
          >En haut</button>
          <button
            class="modal-action {position === 'bas' ? 'primary' : ''}"
            onclick={() => (position = "bas")}
          >En bas</button>
        </div>
      </div>

      <!-- Police -->
      <div class="modal-bloc">
        <div class="modal-bloc-titre">Police</div>
        <select
          class="police-select"
          value={police}
          onchange={(e) => (police = (e.target as HTMLSelectElement).value)}
        >
          {#each POLICES_TITRE as p (p.id)}
            <option value={p.id} style="font-family: '{p.id}', sans-serif;">{p.nom}</option>
          {/each}
        </select>
      </div>

      <!-- Style : gras / italique / souligné -->
      <div class="modal-bloc">
        <div class="modal-bloc-titre">Style</div>
        <div class="style-boutons">
          <button
            class="style-btn {gras ? 'active' : ''}"
            class:gras
            title="Gras"
            onclick={() => (gras = !gras)}
          ><b>G</b></button>
          <button
            class="style-btn {italique ? 'active' : ''}"
            class:italique
            title="Italique"
            onclick={() => (italique = !italique)}
          ><i>I</i></button>
          <button
            class="style-btn {souligne ? 'active' : ''}"
            class:souligne
            title="Souligné"
            onclick={() => (souligne = !souligne)}
          ><u>U</u></button>
        </div>
      </div>

      <!-- Taille -->
      <div class="modal-bloc">
        <div class="modal-bloc-titre">Taille</div>
        <div class="taille-row">
          <input
            class="modal-range"
            type="range"
            min="12"
            max="72"
            step="1"
            value={taille}
            oninput={(e) => (taille = parseFloat((e.target as HTMLInputElement).value))}
          />
          <span class="taille-val">{Math.round(taille)}px</span>
        </div>
      </div>

      <!-- Espacement des lettres -->
      <div class="modal-bloc">
        <div class="modal-bloc-titre">Espacement des lettres</div>
        <div class="taille-row">
          <input
            class="modal-range"
            type="range"
            min="-2"
            max="20"
            step="0.5"
            value={espacement}
            oninput={(e) => (espacement = parseFloat((e.target as HTMLInputElement).value))}
          />
          <span class="taille-val">{espacement}px</span>
        </div>
      </div>

      <!-- Aperçu -->
      <div class="modal-bloc">
        <div class="modal-bloc-titre">Aperçu</div>
        <div class="apercu-zone">
          {#if texte.trim()}
            <div
              class="apercu-titre"
              style="font-family: '{police}', sans-serif; font-size: {taille}px; font-style: {italique ? 'italic' : 'normal'}; font-weight: {gras ? '700' : '400'}; text-decoration: {souligne ? 'underline' : 'none'}; letter-spacing: {espacement}px; background: linear-gradient(135deg, {grad.from}, {grad.to}); -webkit-background-clip: text; background-clip: text; color: transparent;"
            >{texte.trim()}</div>
          {:else}
            <div class="apercu-vide">Le titre apparaîtra ici</div>
          {/if}
        </div>
      </div>

      <!-- Actions -->
      <div class="modal-actions">
        {#if $sceneStore.bgTitre}
          <button class="modal-action danger" onclick={onSupprimer}>Supprimer</button>
        {/if}
        <button class="modal-action" onclick={onAnnuler}>Annuler</button>
        <button class="modal-action primary" onclick={onValider}>Valider</button>
      </div>
    </div>
  </Modal>
{/if}

<style>
  .titre-modal-body {
    padding: 1rem;
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
    overflow-y: auto;
  }
  .titre-input {
    width: 100%;
    padding: 0.5rem 0.7rem;
    background: var(--fond-controle);
    color: var(--texte);
    border: 1px solid var(--bordure);
    font: inherit;
    font-size: 0.9rem;
  }
  .titre-input:focus {
    outline: none;
    border-color: var(--accent-violet);
  }
  .position-boutons {
    display: flex;
    gap: 0.4rem;
  }
  .position-boutons .modal-action {
    flex: 1;
  }
  .style-boutons {
    display: flex;
    gap: 0.4rem;
  }
  .style-btn {
    flex: 0 0 2.2rem;
    height: 2rem;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--btn-surface);
    box-shadow: var(--btn-inset);
    color: var(--texte);
    border: 1px solid var(--bordure);
    font: inherit;
    font-size: 0.95rem;
    cursor: pointer;
    opacity: 0.7;
    transition: background 0.15s ease, box-shadow 0.15s ease, border-color 0.15s ease, opacity 0.15s ease;
  }
  .style-btn:hover {
    background: var(--btn-surface-hover);
    box-shadow: var(--btn-inset-hover);
    border-color: var(--accent-violet);
    opacity: 1;
  }
  .style-btn.active {
    background:
      linear-gradient(
        180deg,
        color-mix(in srgb, var(--accent-violet) 25%, var(--fond-controle)) 0%,
        color-mix(in srgb, var(--accent-violet) 12%, var(--fond-controle)) 100%
      );
    color: var(--texte);
    border-color: var(--accent-violet);
    opacity: 1;
  }
  .police-select {
    width: 100%;
    padding: 0.4rem 0.5rem;
    background: var(--fond-controle);
    color: var(--texte);
    border: 1px solid var(--bordure);
    font: inherit;
    font-size: 0.85rem;
    cursor: pointer;
  }
  .police-select:focus {
    outline: none;
    border-color: var(--accent-violet);
  }
  .police-select option {
    padding: 0.3rem;
    background: var(--fond);
    color: var(--texte);
  }
  .taille-row {
    display: flex;
    align-items: center;
    gap: 0.6rem;
  }
  .taille-row .modal-range {
    flex: 1;
  }
  .taille-val {
    font-size: 0.8rem;
    opacity: 0.8;
    font-variant-numeric: tabular-nums;
    width: 40px;
    text-align: right;
  }
  .apercu-zone {
    display: flex;
    align-items: center;
    justify-content: center;
    min-height: 60px;
    padding: 0.8rem;
    background: var(--fond);
    border: 1px solid var(--bordure);
    border-radius: var(--rayon-petit);
    overflow: hidden;
  }
  .apercu-titre {
    white-space: nowrap;
    line-height: 1.1;
    font-weight: 400;
    text-align: center;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .apercu-vide {
    font-size: 0.8rem;
    opacity: 0.4;
    font-style: italic;
  }
  .modal-actions {
    justify-content: flex-end;
  }
</style>
