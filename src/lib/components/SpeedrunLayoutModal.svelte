<script lang="ts">
  // Modale « Créer votre layout » du splitter speedrun — Layout Editor
  // façon LiveSplit (composants Title / Splits / Timer mappés sur nos
  // éléments). Édite les champs srXxx du widget sélectionné (Option<>
  // camelCase — absent = défaut). Application IMMÉDIATE via
  // setSpeedrunConfig (merge partiel + commit → snapshot WS → la
  // diffusion applique les toggles sans re-render). Ne pilote QUE les
  // éléments internes du splitter : drag/resize, cadre SVG, média de
  // fond et clic sur ligne sont hors de portée.
  import Modal from "./Modal.svelte";
  import {
    selectedIdStore,
    sceneStore,
    setSpeedrunConfig,
    setSpeedrunPolice,
    setSpeedrunPoliceLocal,
    commitScene,
  } from "../stores/scene";
  import { POLICES_TITRE } from "../fonts";
  import { t } from "../stores/langue";
  import { tauri } from "../tauri";

  let { onFermer }: { onFermer: () => void } = $props();

  let selectedId = $derived($selectedIdStore);
  let w = $derived($sceneStore.widgets.find((x) => x.id === selectedId) ?? null);

  type Patch = Parameters<typeof setSpeedrunConfig>[1];
  function patch(p: Patch) {
    if (selectedId) void setSpeedrunConfig(selectedId, p);
  }
  function onToggle(key: keyof Patch) {
    return (e: Event) => patch({ [key]: (e.target as HTMLInputElement).checked } as Patch);
  }
  function onSelect(key: keyof Patch) {
    return (e: Event) => patch({ [key]: (e.target as HTMLSelectElement).value } as Patch);
  }

  // ===== Général : disposition / police / taille (mêmes setters que la
  // carte d'édition — les deux surfaces restent synchronisées). =====
  let policeVal = $derived(w?.srPolice ?? "Rajdhani");
  let tailleVal = $derived(w?.srTaillePolice ?? 27);
  function onPoliceChange(e: Event) {
    if (!selectedId) return;
    void setSpeedrunPolice(selectedId, (e.target as HTMLSelectElement).value, tailleVal);
  }
  function onTailleInput(e: Event) {
    if (!selectedId) return;
    setSpeedrunPoliceLocal(selectedId, policeVal, parseFloat((e.target as HTMLInputElement).value));
  }
  async function onTailleChange() { await commitScene(); }

  // ===== Couleurs : input color = écriture directe du hex ; le bouton
  // « auto » retire la clé (undefined → omis à la sérialisation → retour
  // aux variables sémantiques). Valeurs affichées quand non définies :
  // placeholders neutres (les vraies valeurs vivent en variables CSS). =====
  function onCouleur(key: "srCouleurTimer" | "srCouleurAhead" | "srCouleurBehind" | "srCouleurActif") {
    return (e: Event) => patch({ [key]: (e.target as HTMLInputElement).value } as Patch);
  }
  function resetCouleur(key: "srCouleurTimer" | "srCouleurAhead" | "srCouleurBehind" | "srCouleurActif") {
    return () => patch({ [key]: undefined } as Patch);
  }

  // Nombres : 0/vide = défaut → on retire la clé (undefined) plutôt que
  // d'écrire 0 (sauf épaisseur de bordure qui a un vrai défaut numérique).
  function onNumAuto(key: "srSegBordureRayon" | "srNbLignes" | "srHauteurLigne") {
    return (e: Event) => {
      const v = +(e.target as HTMLInputElement).value;
      patch({ [key]: v > 0 ? Math.round(v) : undefined } as Patch);
    };
  }

  // ===== Points de validation : son joué côté diffusion à l'activation
  // d'un point (import via import_son — même mécanique que les sons des
  // alertes). Retourne le chemin relatif "medias/<uuid>.<ext>" ou null. =====
  async function onImporterSonPoints(): Promise<void> {
    try {
      const rel = await tauri.importSon();
      if (rel) patch({ srPointsSon: rel });
    } catch {
      // Dialog annulé / import refusé — rien à faire.
    }
  }
</script>

<Modal
  title={$t("speedrunLayout.titre")}
  onClose={onFermer}
  hint={$t("speedrunLayout.hint")}
  maxWidth="34rem"
>
  <div class="srl-corps">
    {#if !w || w.type !== "speedrun"}
      <p class="hint">{$t("speedrunLayout.aucunWidget")}</p>
    {:else}
      <!-- ===== Général ===== -->
      <div class="modal-bloc">
        <div class="bloc-titre">{$t("speedrunLayout.secGeneral")}</div>
        <div class="pos-grid">
          <label class="pos-field">
            <span class="field-label">{$t("speedrunLayout.disposition")}</span>
            <select value={w.srLayout ?? "auto"} onchange={onSelect("srLayout")}>
              <option value="auto">{$t("widgetSpeedrun.dispoAuto")}</option>
              <option value="colonne">{$t("widgetSpeedrun.dispoColonne")}</option>
              <option value="ligne">{$t("widgetSpeedrun.dispoLigne")}</option>
            </select>
          </label>
          <label class="pos-field">
            <span class="field-label">{$t("commun.police")}</span>
            <select class="police-select" value={policeVal} onchange={onPoliceChange}>
              {#each POLICES_TITRE as p (p.id)}
                <option value={p.id} style="font-family: '{p.id}', sans-serif;">{p.nom}</option>
              {/each}
            </select>
          </label>
          <label class="pos-field">
            <span class="field-label">{$t("speedrunLayout.taillePolice")} — {tailleVal}px</span>
            <input class="modal-range" type="range" min="8" max="48" step="1"
              value={tailleVal} oninput={onTailleInput} onchange={onTailleChange} />
          </label>
        </div>
      </div>

      <!-- ===== En-tête ===== -->
      <div class="modal-bloc">
        <div class="bloc-titre">{$t("speedrunLayout.secEntete")}</div>
        <label class="toggle">
          <input type="checkbox" checked={w.srVoirTitre !== false} onchange={onToggle("srVoirTitre")} />
          <span>{$t("speedrunLayout.titreJeu")}</span>
        </label>
        <label class="toggle">
          <input type="checkbox" checked={w.srVoirCategorie !== false} onchange={onToggle("srVoirCategorie")} />
          <span>{$t("speedrunLayout.categorie")}</span>
        </label>
        <label class="toggle">
          <input type="checkbox" checked={w.srVoirStatut !== false} onchange={onToggle("srVoirStatut")} />
          <span>{$t("speedrunLayout.pastilleStatut")}</span>
        </label>
      </div>

      <!-- ===== Timer ===== -->
      <div class="modal-bloc">
        <div class="bloc-titre">{$t("speedrunLayout.secTimer")}</div>
        <label class="toggle">
          <input type="checkbox" checked={w.srVoirTimer !== false} onchange={onToggle("srVoirTimer")} />
          <span>{$t("speedrunLayout.timerVisible")}</span>
        </label>
        <div class="pos-grid">
          <label class="pos-field">
            <span class="field-label">{$t("speedrunLayout.timerPrecision")}</span>
            <select value={w.srTimerPrecision ?? "centieme"} onchange={onSelect("srTimerPrecision")}>
              <option value="seconde">{$t("speedrunLayout.precSecondes")}</option>
              <option value="dixieme">{$t("speedrunLayout.precDixiemes")}</option>
              <option value="centieme">{$t("speedrunLayout.precCentiemes")}</option>
            </select>
          </label>
          <label class="pos-field">
            <span class="field-label">{$t("speedrunLayout.timerMethode")}</span>
            <select value={w.srTimerMethode ?? "auto"} onchange={onSelect("srTimerMethode")}>
              <option value="auto">{$t("speedrunLayout.methodeAuto")}</option>
              <option value="real">{$t("speedrunLayout.methodeReal")}</option>
              <option value="game">{$t("speedrunLayout.methodeGame")}</option>
            </select>
          </label>
          <div class="pos-field">
            <span class="field-label">{$t("speedrunLayout.timerCouleur")}</span>
            <div class="couleur-row">
              <input type="color" value={w.srCouleurTimer || "#e0e0e0"} oninput={onCouleur("srCouleurTimer")} />
              {#if w.srCouleurTimer}
                <button type="button" class="compacte-lien" onclick={resetCouleur("srCouleurTimer")}>{$t("speedrunLayout.couleurAuto")}</button>
              {/if}
            </div>
          </div>
        </div>
      </div>

      <!-- ===== Méta ===== -->
      <div class="modal-bloc">
        <div class="bloc-titre">{$t("speedrunLayout.secMeta")}</div>
        <label class="toggle">
          <input type="checkbox" checked={w.srVoirPhase !== false} onchange={onToggle("srVoirPhase")} />
          <span>{$t("speedrunLayout.phasePill")}</span>
        </label>
        <label class="toggle">
          <input type="checkbox" checked={w.srVoirCompteur !== false} onchange={onToggle("srVoirCompteur")} />
          <span>{$t("speedrunLayout.compteur")}</span>
        </label>
        <label class="toggle">
          <input type="checkbox" checked={w.srVoirBarre !== false} onchange={onToggle("srVoirBarre")} />
          <span>{$t("speedrunLayout.barre")}</span>
        </label>
      </div>

      <!-- ===== Segments ===== -->
      <div class="modal-bloc">
        <div class="bloc-titre">{$t("speedrunLayout.secSegments")}</div>
        <div class="pos-grid">
          <label class="pos-field">
            <span class="field-label">{$t("speedrunLayout.colTemps")}</span>
            <select value={w.srModeTemps ?? "cumul"} onchange={onSelect("srModeTemps")}>
              <option value="cumul">{$t("speedrunLayout.tempsCumul")}</option>
              <option value="segment">{$t("speedrunLayout.tempsSegment")}</option>
            </select>
          </label>
          <label class="pos-field">
            <span class="field-label">{$t("speedrunLayout.colDelta")}</span>
            <select value={w.srModeDelta ?? "cumul"} onchange={onSelect("srModeDelta")}>
              <option value="cumul">{$t("speedrunLayout.deltaCumul")}</option>
              <option value="segment">{$t("speedrunLayout.deltaSegment")}</option>
              <option value="off">{$t("speedrunLayout.deltaOff")}</option>
            </select>
          </label>
          <label class="pos-field">
            <span class="field-label">{$t("speedrunLayout.deltaPrecision")}</span>
            <select value={w.srDeltaPrecision ?? "dixieme"} onchange={onSelect("srDeltaPrecision")}>
              <option value="dixieme">{$t("speedrunLayout.precDixiemes")}</option>
              <option value="centieme">{$t("speedrunLayout.precCentiemes")}</option>
              <option value="seconde">{$t("speedrunLayout.precSecondes")}</option>
            </select>
          </label>
        </div>
        <label class="toggle">
          <input type="checkbox" checked={w.srDeltaDrop !== false} onchange={onToggle("srDeltaDrop")} />
          <span>{$t("speedrunLayout.deltaDrop")}</span>
        </label>

        <label class="toggle">
          <input type="checkbox" checked={w.srSegBordure !== false} onchange={onToggle("srSegBordure")} />
          <span>{$t("speedrunLayout.segBordure")}</span>
        </label>
        <div class="pos-grid">
          <label class="pos-field">
            <span class="field-label">{$t("speedrunLayout.segBordEp")}</span>
            <input type="number" min="0" max="10" step="1" value={w.srSegBordureEpaisseur ?? 1}
              onchange={(e) => patch({ srSegBordureEpaisseur: Math.max(0, Math.round(+(e.target as HTMLInputElement).value)) })} />
          </label>
          <label class="pos-field">
            <span class="field-label">{$t("speedrunLayout.segBordRayon")}</span>
            <input type="number" min="0" max="40" step="1" placeholder="auto" value={w.srSegBordureRayon ?? ""}
              onchange={onNumAuto("srSegBordureRayon")} />
          </label>
          <div class="pos-field">
            <span class="field-label">{$t("speedrunLayout.couleurAhead")}</span>
            <div class="couleur-row">
              <input type="color" value={w.srCouleurAhead || "#4ade80"} oninput={onCouleur("srCouleurAhead")} />
              {#if w.srCouleurAhead}
                <button type="button" class="compacte-lien" onclick={resetCouleur("srCouleurAhead")}>{$t("speedrunLayout.couleurAuto")}</button>
              {/if}
            </div>
          </div>
          <div class="pos-field">
            <span class="field-label">{$t("speedrunLayout.couleurBehind")}</span>
            <div class="couleur-row">
              <input type="color" value={w.srCouleurBehind || "#f87171"} oninput={onCouleur("srCouleurBehind")} />
              {#if w.srCouleurBehind}
                <button type="button" class="compacte-lien" onclick={resetCouleur("srCouleurBehind")}>{$t("speedrunLayout.couleurAuto")}</button>
              {/if}
            </div>
          </div>
          <div class="pos-field">
            <span class="field-label">{$t("speedrunLayout.couleurActif")}</span>
            <div class="couleur-row">
              <input type="color" value={w.srCouleurActif || "#e0e0e0"} oninput={onCouleur("srCouleurActif")} />
              {#if w.srCouleurActif}
                <button type="button" class="compacte-lien" onclick={resetCouleur("srCouleurActif")}>{$t("speedrunLayout.couleurAuto")}</button>
              {/if}
            </div>
          </div>
        </div>

        <div class="pos-grid">
          <label class="pos-field">
            <span class="field-label">{$t("speedrunLayout.nbLignes")}</span>
            <input type="number" min="0" max="99" step="1" placeholder="0" value={w.srNbLignes ?? ""}
              onchange={onNumAuto("srNbLignes")} />
          </label>
          <label class="pos-field">
            <span class="field-label">{$t("speedrunLayout.hauteurLigne")}</span>
            <input type="number" min="0" max="200" step="1" placeholder="auto" value={w.srHauteurLigne ?? ""}
              onchange={onNumAuto("srHauteurLigne")} />
          </label>
        </div>
        <label class="toggle">
          <input type="checkbox" checked={w.srScrollAuto !== false} onchange={onToggle("srScrollAuto")} />
          <span>{$t("speedrunLayout.scrollAuto")}</span>
        </label>
      </div>

      <!-- ===== Points de validation ===== -->
      <div class="modal-bloc">
        <div class="bloc-titre">{$t("speedrunLayout.secPoints")}</div>
        <label class="toggle">
          <input type="checkbox" checked={w.srVoirPoints !== false} onchange={onToggle("srVoirPoints")} />
          <span>{$t("speedrunLayout.pointsVisibles")}</span>
        </label>
        <p class="pts-hint">{$t("speedrunLayout.pointsHint")}</p>
        <div class="pos-grid">
          <label class="pos-field">
            <span class="field-label">{$t("speedrunLayout.pointsDureePop")}</span>
            <input type="number" min="1" max="30" step="1" value={w.srPointsDureePop ?? 2}
              onchange={(e) => {
                const v = Math.round(+(e.target as HTMLInputElement).value);
                patch({ srPointsDureePop: v >= 1 ? v : undefined });
              }} />
          </label>
          <label class="pos-field">
            <span class="field-label">{$t("speedrunLayout.pointsSonVolume", { n: Math.round((w.srPointsSonVolume ?? 1) * 100) })}</span>
            <input class="modal-range" type="range" min="0" max="100" step="5"
              value={Math.round((w.srPointsSonVolume ?? 1) * 100)}
              onchange={(e) => patch({ srPointsSonVolume: +(e.target as HTMLInputElement).value / 100 })} />
          </label>
        </div>
        <div class="pos-field">
          <span class="field-label">{$t("speedrunLayout.pointsSon")}</span>
          <div class="couleur-row">
            {#if w.srPointsSon}
              <span class="pts-son-nom">{w.srPointsSon.split("/").pop()}</span>
              <button type="button" class="compacte-lien" onclick={() => patch({ srPointsSon: undefined })}>{$t("speedrunLayout.pointsSonRetirer")}</button>
            {:else}
              <button type="button" class="compacte-lien" onclick={() => void onImporterSonPoints()}>{$t("speedrunLayout.pointsSonImporter")}</button>
            {/if}
          </div>
        </div>
      </div>
    {/if}
  </div>
</Modal>

<style>
  .srl-corps {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 0.8rem 1rem;
    display: flex;
    flex-direction: column;
    gap: 0.7rem;
  }
  .modal-bloc {
    display: flex;
    flex-direction: column;
    gap: 0.45rem;
    padding: 0.55rem 0.7rem;
    border: 1px solid var(--bordure-ui);
    border-radius: var(--rayon-petit);
    background: color-mix(in srgb, var(--fond-controle) 40%, transparent);
  }
  .bloc-titre {
    font-size: 0.75rem;
    font-weight: 700;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    opacity: 0.8;
  }
  .toggle {
    display: flex;
    align-items: center;
    gap: 0.45rem;
    font-size: 0.82rem;
    cursor: pointer;
  }
  .pos-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(8.5rem, 1fr));
    gap: 0.5rem;
  }
  .pos-field {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    min-width: 0;
  }
  .pos-field input[type="number"],
  .pos-field select {
    width: 100%;
  }
  .police-select {
    width: 100%;
  }
  /* Inputs nombre : recette commune des modales (fond contrôle +
     bordure neutre, accent au focus) — app.css ne couvre que les selects. */
  .pos-field input[type="number"] {
    background: var(--fond-controle);
    color: var(--texte);
    border: 1px solid var(--bordure-ui);
    border-radius: var(--rayon-petit);
    padding: 0.3rem 0.45rem;
    font: inherit;
    font-size: 0.85rem;
  }
  .pos-field input[type="number"]:focus {
    outline: none;
    border-color: var(--accent-ui);
  }
  .couleur-row {
    display: flex;
    align-items: center;
    gap: 0.45rem;
  }
  .couleur-row input[type="color"] {
    width: 2.2rem;
    height: 1.6rem;
    padding: 0;
    border: 1px solid var(--bordure-ui);
    border-radius: var(--rayon-petit);
    background: var(--fond-controle);
    cursor: pointer;
  }
  .compacte-lien {
    background: none;
    border: none;
    color: var(--texte);
    opacity: 0.55;
    font: inherit;
    font-size: 0.68rem;
    padding: 0;
    cursor: pointer;
    white-space: nowrap;
    transition: opacity 0.15s ease;
  }
  .compacte-lien:hover {
    opacity: 0.95;
    text-decoration: underline;
  }
  .pts-hint {
    font-size: 0.72rem;
    opacity: 0.6;
    margin: 0;
  }
  .pts-son-nom {
    font-size: 0.78rem;
    opacity: 0.8;
    word-break: break-all;
  }
</style>
