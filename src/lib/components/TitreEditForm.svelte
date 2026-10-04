<script lang="ts">
  // TitreEditForm — form partagé des modales de titres (widget + fond).
  // 4 textes par position (haut/bas/gauche/droite — champ vide = aucun titre
  // à cette position), style partagé (police/taille/…/contour) et aperçu
  // mini-canvas avec les textes ancrés à leur position (latéraux verticaux).
  // Champs bindables : la modale parente possède l'état local (commit
  // seulement au Valider — pattern des anciennes modales titre).
  import { POLICES_TITRE, POLICE_TITRE_DEFAUT, TAILLE_TITRE_DEFAUT, ESPACEMENT_TITRE_DEFAUT } from "../fonts";
  import { t } from "../stores/langue";
  import type { PositionTitre } from "../stores/ui";

  let {
    haut = $bindable(""),
    bas = $bindable(""),
    gauche = $bindable(""),
    droite = $bindable(""),
    police = $bindable(POLICE_TITRE_DEFAUT),
    taille = $bindable(TAILLE_TITRE_DEFAUT),
    gras = $bindable(false),
    italique = $bindable(false),
    souligne = $bindable(false),
    espacement = $bindable(ESPACEMENT_TITRE_DEFAUT),
    contourCouleur = $bindable("#ffffff"),
    contourEpaisseur = $bindable(2),
    /// Dégradé personnalisé du texte. undefined = auto (dégradé du cadre).
    couleur = $bindable<string | undefined>(undefined),
    couleurFin = $bindable<string | undefined>(undefined),
    /// Gradient du cadre actif pour l'aperçu : { from, to }.
    grad,
    /// Position à focus à l'ouverture (bord double-cliqué sur le widget).
    focusPosition = "",
  }: {
    haut?: string;
    bas?: string;
    gauche?: string;
    droite?: string;
    police?: string;
    taille?: number;
    gras?: boolean;
    italique?: boolean;
    souligne?: boolean;
    espacement?: number;
    contourCouleur?: string;
    contourEpaisseur?: number;
    couleur?: string | undefined;
    couleurFin?: string | undefined;
    grad: { from: string; to: string };
    focusPosition?: PositionTitre | "";
  } = $props();

  // Focus du champ du bord double-cliqué à l'ouverture de la modale.
  let posInputs: Record<string, HTMLInputElement> = {};
  $effect(() => {
    if (focusPosition && posInputs[focusPosition]) posInputs[focusPosition].focus();
  });

  // Au moins un texte non vide ? (aperçu)
  let texteQq = $derived(!!(haut.trim() || bas.trim() || gauche.trim() || droite.trim()));

  // Dégradé effectif du texte : personnalisé si couleur définie, sinon
  // celui du cadre actif (auto).
  let gradEff = $derived(
    couleur != null ? { from: couleur, to: couleurFin ?? couleur } : grad
  );

  // Style commun des textes de l'aperçu (gradient effectif, taille
  // plafonnée pour tenir dans le mini-canvas).
  let apercuStyle = $derived(
    "font-family: '" + police + "', sans-serif; font-size: " + Math.min(taille, 42) +
    "px; font-style: " + (italique ? "italic" : "normal") +
    "; font-weight: " + (gras ? "700" : "400") +
    "; text-decoration: " + (souligne ? "underline" : "none") +
    "; letter-spacing: " + espacement + "px; -webkit-text-stroke: " + contourEpaisseur +
    "px " + contourCouleur + "; paint-order: stroke fill; background: linear-gradient(135deg, " +
    gradEff.from + ", " + gradEff.to + "); -webkit-background-clip: text; background-clip: text; color: transparent;"
  );
</script>

<!-- Textes par position (champ vide = pas de titre à cette position) -->
<div class="modal-bloc">
  <div class="modal-bloc-titre">{$t("titreForm.textesParPosition")}</div>
  <div class="pos-hint">{$t("titreForm.posHint")}</div>
  <div class="pos-grid">
    <label class="pos-field">
      <span class="pos-label">{$t("titreForm.enHaut")}</span>
      <input
        class="titre-input"
        bind:this={posInputs.haut}
        type="text"
        maxlength="60"
        placeholder={$t("titreForm.placeholder")}
        bind:value={haut}
      />
    </label>
    <label class="pos-field">
      <span class="pos-label">{$t("titreForm.enBas")}</span>
      <input
        class="titre-input"
        bind:this={posInputs.bas}
        type="text"
        maxlength="60"
        placeholder={$t("titreForm.placeholder")}
        bind:value={bas}
      />
    </label>
    <label class="pos-field">
      <span class="pos-label">{$t("titreForm.aGauche")} <em>{$t("titreForm.vertical")}</em></span>
      <input
        class="titre-input"
        bind:this={posInputs.gauche}
        type="text"
        maxlength="60"
        placeholder={$t("titreForm.placeholder")}
        bind:value={gauche}
      />
    </label>
    <label class="pos-field">
      <span class="pos-label">{$t("titreForm.aDroite")} <em>{$t("titreForm.vertical")}</em></span>
      <input
        class="titre-input"
        bind:this={posInputs.droite}
        type="text"
        maxlength="60"
        placeholder={$t("titreForm.placeholder")}
        bind:value={droite}
      />
    </label>
  </div>
</div>

<!-- Police -->
<div class="modal-bloc">
  <div class="modal-bloc-titre">{$t("commun.police")}</div>
  <select
    class="police-select"
    bind:value={police}
  >
    {#each POLICES_TITRE as p (p.id)}
      <option value={p.id} style="font-family: '{p.id}', sans-serif;">{p.nom}</option>
    {/each}
  </select>
</div>

<!-- Style : gras / italique / souligné -->
<div class="modal-bloc">
  <div class="modal-bloc-titre">{$t("titreForm.style")}</div>
  <div class="style-boutons">
    <button
      class="style-btn {gras ? 'active' : ''}"
      class:gras
      title={$t("titreForm.gras")}
      onclick={() => (gras = !gras)}
    ><b>G</b></button>
    <button
      class="style-btn {italique ? 'active' : ''}"
      class:italique
      title={$t("titreForm.italique")}
      onclick={() => (italique = !italique)}
    ><i>I</i></button>
    <button
      class="style-btn {souligne ? 'active' : ''}"
      class:souligne
      title={$t("titreForm.souligne")}
      onclick={() => (souligne = !souligne)}
    ><u>U</u></button>
  </div>
</div>

<!-- Taille -->
<div class="modal-bloc">
  <div class="modal-bloc-titre">{$t("commun.taille")}</div>
  <div class="taille-row">
    <input
      class="modal-range"
      type="range"
      min="12"
      max="72"
      step="1"
      bind:value={taille}
    />
    <span class="taille-val">{Math.round(taille)}px</span>
  </div>
</div>

<!-- Espacement des lettres -->
<div class="modal-bloc">
  <div class="modal-bloc-titre">{$t("titreForm.espacement")}</div>
  <div class="taille-row">
    <input
      class="modal-range"
      type="range"
      min="-2"
      max="20"
      step="0.5"
      bind:value={espacement}
    />
    <span class="taille-val">{espacement}px</span>
  </div>
</div>

<!-- Couleur du texte : dégradé personnalisable (auto = dégradé du cadre) -->
<div class="modal-bloc">
  <div class="modal-bloc-titre">{$t("titreForm.couleurTexte")}</div>
  <div class="taille-row">
    <span class="lbl">{$t("commun.debut")}</span>
    <input
      class="couleur-input"
      type="color"
      title={$t("titreForm.couleurDebutTitle")}
      value={couleur ?? grad.from}
      disabled={couleur == null}
      oninput={(e) => { couleur = e.currentTarget.value; }}
    />
    <span class="lbl">{$t("commun.fin")}</span>
    <input
      class="couleur-input"
      type="color"
      title={$t("titreForm.couleurFinTitle")}
      value={couleurFin ?? couleur ?? grad.to}
      disabled={couleur == null}
      oninput={(e) => { couleurFin = e.currentTarget.value; }}
    />
    {#if couleur != null}
      <button
        class="auto-btn"
        title={$t("titreForm.autoCadreTitle")}
        onclick={() => { couleur = undefined; couleurFin = undefined; }}
      >{$t("titreForm.autoCadre")}</button>
    {:else}
      <button
        class="auto-btn"
        title={$t("titreForm.personnaliserTitle")}
        onclick={() => { couleur = grad.from; couleurFin = grad.to; }}
      >{$t("titreForm.personnaliser")}</button>
      <span class="auto-tag">{$t("titreForm.autoTag")}</span>
    {/if}
  </div>
</div>

<!-- Contour des lettres : couleur + épaisseur (0 = aucun contour) -->
<div class="modal-bloc">
  <div class="modal-bloc-titre">{$t("titreForm.contour")}</div>
  <div class="taille-row">
    <input
      class="couleur-input"
      type="color"
      title={$t("titreForm.contourCouleurTitle")}
      bind:value={contourCouleur}
    />
    <input
      class="modal-range"
      type="range"
      min="0"
      max="6"
      step="1"
      title={$t("titreForm.contourEpaisseurTitle")}
      bind:value={contourEpaisseur}
    />
    <span class="taille-val">{contourEpaisseur === 0 ? $t("commun.aucun") : Math.round(contourEpaisseur) + "px"}</span>
  </div>
</div>

<!-- Aperçu mini-canvas : les textes ancrés à leur position -->
<div class="modal-bloc">
  <div class="modal-bloc-titre">{$t("commun.apercu")}</div>
  <div class="apercu-zone">
    {#if texteQq}
      <div class="apercu-canvas">
        {#if haut.trim()}<div class="apercu-txt pos-haut" style={apercuStyle}>{haut.trim()}</div>{/if}
        {#if bas.trim()}<div class="apercu-txt pos-bas" style={apercuStyle}>{bas.trim()}</div>{/if}
        {#if gauche.trim()}<div class="apercu-txt pos-gauche" style={apercuStyle}>{gauche.trim()}</div>{/if}
        {#if droite.trim()}<div class="apercu-txt pos-droite" style={apercuStyle}>{droite.trim()}</div>{/if}
      </div>
    {:else}
      <div class="apercu-vide">{$t("titreForm.apercuVide")}</div>
    {/if}
  </div>
</div>

<style>
  .pos-hint {
    font-size: 0.78rem;
    opacity: 0.65;
    margin-bottom: 0.45rem;
  }
  .pos-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 0.5rem;
  }
  .pos-field {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    min-width: 0;
  }
  .pos-label {
    font-size: 0.78rem;
    opacity: 0.8;
  }
  .pos-label em {
    font-style: italic;
    opacity: 0.7;
  }
  .titre-input {
    width: 100%;
    padding: 0.5rem 0.7rem;
    background: var(--fond-controle);
    color: var(--texte);
    border: 1px solid var(--bordure-ui);
    font: inherit;
    font-size: 0.9rem;
  }
  .titre-input:focus {
    outline: none;
    border-color: var(--accent-ui);
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
    border: 1px solid var(--bordure-ui);
    font: inherit;
    font-size: 0.95rem;
    cursor: pointer;
    opacity: 0.7;
    transition: background 0.15s ease, box-shadow 0.15s ease, border-color 0.15s ease, opacity 0.15s ease;
  }
  .style-btn:hover {
    background: var(--btn-surface-hover);
    box-shadow: var(--btn-inset-hover);
    border-color: var(--accent-ui);
    opacity: 1;
  }
  .style-btn.active {
    background:
      linear-gradient(
        180deg,
        color-mix(in srgb, var(--accent-ui) 25%, var(--fond-controle)) 0%,
        color-mix(in srgb, var(--accent-ui) 12%, var(--fond-controle)) 100%
      );
    color: var(--texte);
    border-color: var(--accent-ui);
    opacity: 1;
  }
  .police-select {
    width: 100%;
    padding: 0.4rem 0.5rem;
    background: var(--fond-controle);
    color: var(--texte);
    border: 1px solid var(--bordure-ui);
    font: inherit;
    font-size: 0.85rem;
    cursor: pointer;
  }
  .police-select:focus {
    outline: none;
    border-color: var(--accent-ui);
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
  .couleur-input {
    width: 28px;
    height: 24px;
    border: 1px solid var(--bordure-ui);
    background: transparent;
    cursor: pointer;
    flex-shrink: 0;
    padding: 0;
  }
  .couleur-input:disabled {
    opacity: 0.45;
    cursor: default;
  }
  .lbl {
    font-size: 0.78rem;
    opacity: 0.8;
    flex-shrink: 0;
  }
  .auto-btn {
    padding: 0.25rem 0.6rem;
    background: var(--btn-surface);
    box-shadow: var(--btn-inset);
    color: var(--texte);
    border: 1px solid var(--bordure-ui);
    font: inherit;
    font-size: 0.78rem;
    cursor: pointer;
    flex-shrink: 0;
    opacity: 0.85;
    transition: background 0.15s ease, border-color 0.15s ease, opacity 0.15s ease;
  }
  .auto-btn:hover {
    background: var(--btn-surface-hover);
    border-color: var(--accent-ui);
    opacity: 1;
  }
  .auto-tag {
    font-size: 0.75rem;
    font-style: italic;
    opacity: 0.55;
  }
  .apercu-zone {
    display: flex;
    align-items: center;
    justify-content: center;
    min-height: 60px;
    padding: 0.4rem;
    background: var(--fond);
    border: 1px solid var(--bordure-ui);
    border-radius: var(--rayon-petit);
    overflow: hidden;
  }
  .apercu-canvas {
    position: relative;
    width: 100%;
    aspect-ratio: 16 / 9;
    overflow: hidden;
  }
  .apercu-txt {
    position: absolute;
    white-space: nowrap;
    line-height: 1.1;
    font-weight: 400;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .apercu-txt.pos-haut {
    top: 4px;
    left: 50%;
    transform: translateX(-50%);
    max-width: 90%;
  }
  .apercu-txt.pos-bas {
    bottom: 4px;
    left: 50%;
    transform: translateX(-50%);
    max-width: 90%;
  }
  .apercu-txt.pos-gauche {
    left: 4px;
    top: 50%;
    transform: translateY(-50%);
    writing-mode: vertical-rl;
    max-height: 90%;
  }
  .apercu-txt.pos-droite {
    right: 4px;
    top: 50%;
    transform: translateY(-50%);
    writing-mode: vertical-rl;
    max-height: 90%;
  }
  .apercu-vide {
    font-size: 0.8rem;
    opacity: 0.4;
    font-style: italic;
  }
</style>
