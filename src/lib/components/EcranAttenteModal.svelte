<script lang="ts">
  // EcranAttenteModal — modale de config/contrôle de l'écran d'attente (BRB).
  // Ouverte par le lanceur « Écran d'attente » de la Toolbar + la pillule
  // header quand l'écran est actif + la barre de raccourcis.
  //
  // CONSIGNE UTILISATEUR : le média (image/vidéo) n'est JAMAIS rendu ici —
  // affichage et son sont portés par la diffusion :4321 uniquement. La modale
  // ne montre que le NOM du fichier importé, pas son contenu.
  //
  // Le chrono est détenu par Rust (tâche tokio 1 Hz) : temps_restant arrive
  // via l'event `ecran-attente:etat` — la modale ne fait que l'afficher.
  import { get } from "svelte/store";
  import { ecranAttenteModalOpen } from "../stores/ui";
  import { sceneStore } from "../stores/scene";
  import {
    ecranAttenteStore,
    ecranAttenteErreur,
    initEcranAttente,
    chargerEcranAttente,
    ecranAttenteSetConfig,
    lancerEcranAttente,
    togglePauseEcranAttente,
    arreterEcranAttente,
    importerMediaEcranAttente,
    retirerMediaEcranAttente,
    listerSourcesAudioEcranAttente,
  } from "../stores/ecranAttente";
  import { obsStatus } from "../stores/obs";
  import type { EcranAttenteConfig } from "../tauri";
  import Modal from "./Modal.svelte";

  // Valeurs locales éditables (sync depuis la config à l'ouverture ; commit
  // immédiat par champ — les toggles/ranges s'appliquent en live côté
  // diffusion via la frame ecran-attente-etat re-émise).
  let dureeMin = $state(10);
  let dureeSec = $state(0);
  let titre = $state("");
  let afficherTitre = $state(true);
  let son = $state(true);
  let volume = $state(50);
  let sourceAudio = $state("");

  // Position/échelle de la carte minuteur (centre x/y en px canvas + échelle).
  // Éditée via le mini-canvas ci-dessous (drag = déplacer, coin = resize) —
  // même mécanique que le « Squelette de position » d'InteractionViewerModal.
  let mx = $state(960);
  let my = $state(540);
  let mechelle = $state(1);

  // Picker de source audio OBS (liste chargée à l'ouverture, non-fatal).
  let sourcesAudio = $state<string[]>([]);
  let sourcesErreur = $state<string | null>(null);

  // Action en cours (désactive les boutons pendant l'invoke).
  let enCours = $state(false);

  let etat = $derived($ecranAttenteStore);
  let actif = $derived(etat?.actif ?? false);
  let enPause = $derived(etat?.en_pause ?? false);
  let mediaNom = $derived(etat?.config.media_nom ?? null);
  let mediaKind = $derived(etat?.config.media_kind ?? null);

  /// Recopie la config du store dans les champs locaux.
  function syncLocals(c: EcranAttenteConfig) {
    dureeMin = Math.floor(c.duree_s / 60);
    dureeSec = Math.round(c.duree_s % 60);
    titre = c.titre;
    afficherTitre = c.afficher_titre;
    son = c.son;
    volume = Math.round(c.volume * 100);
    sourceAudio = c.source_audio ?? "";
    mx = c.minuteur_x;
    my = c.minuteur_y;
    mechelle = c.minuteur_echelle;
  }

  // À l'ouverture : init listeners + chargement état + sync champs + liste
  // des sources audio OBS. get() au lieu de $store → l'effect ne dépend pas
  // du store (sinon chaque tick 1 Hz réinitialiserait les champs en cours
  // de saisie).
  $effect(() => {
    if ($ecranAttenteModalOpen) {
      void initEcranAttente();
      const c = get(ecranAttenteStore)?.config;
      if (c) syncLocals(c);
      void chargerEcranAttente().then(() => {
        const c2 = get(ecranAttenteStore)?.config;
        if (c2) syncLocals(c2);
      });
      void refreshSourcesAudio();
    }
  });

  /// Commit immédiat d'un champ de config (persiste + re-émet la frame WS
  /// si l'écran est actif → changement visible en live sur la diffusion).
  async function commitConfig(patch: Partial<EcranAttenteConfig>): Promise<void> {
    const cur = get(ecranAttenteStore)?.config;
    if (!cur) return;
    try {
      await ecranAttenteSetConfig({ ...cur, ...patch });
      ecranAttenteErreur.set(null);
    } catch (e) {
      ecranAttenteErreur.set(String(e));
    }
  }

  function commitDuree() {
    const total = Math.max(1, dureeMin * 60 + dureeSec);
    void commitConfig({ duree_s: total });
  }

  // ===== Mini-canvas : drag/resize de la carte minuteur =====
  // Canvas de référence = la scène courante (fallback 1920×1080). Le
  // squelette représente la carte minuteur (~320×170 px à échelle 1) ;
  // left/top stockés = CENTRE de la carte (même convention que diffusion).
  let miniW = $state(300);
  let canvasW = $derived($sceneStore.canvasW ?? 1920);
  let canvasH = $derived($sceneStore.canvasH ?? 1080);
  let miniScale = $derived(miniW / canvasW);
  // Taille approximative de la carte à l'échelle courante (repère visuel —
  // ~380×160 px à échelle 1 avec le padding 24/64 de la diffusion).
  let skelW = $derived(380 * mechelle);
  let skelH = $derived(160 * mechelle);

  type EaDragMode = "move" | "resize" | null;
  let eaDragMode = $state<EaDragMode>(null);
  let eaDragStart = { px: 0, py: 0, mx: 0, my: 0, echelle: 1 };

  function eaOnRectDown(e: PointerEvent) {
    e.stopPropagation();
    e.preventDefault();
    eaDragMode = "move";
    eaDragStart = { px: e.clientX, py: e.clientY, mx, my, echelle: mechelle };
    (e.target as HTMLElement).setPointerCapture(e.pointerId);
  }

  function eaOnHandleDown(e: PointerEvent) {
    e.stopPropagation();
    e.preventDefault();
    eaDragMode = "resize";
    eaDragStart = { px: e.clientX, py: e.clientY, mx, my, echelle: mechelle };
    (e.target as HTMLElement).setPointerCapture(e.pointerId);
  }

  function eaOnPointerMove(e: PointerEvent) {
    if (!eaDragMode) return;
    const dx = (e.clientX - eaDragStart.px) / miniScale;
    const dy = (e.clientY - eaDragStart.py) / miniScale;

    if (eaDragMode === "move") {
      // Centre borné pour que la carte reste entièrement dans le canvas.
      mx = Math.max(skelW / 2, Math.min(canvasW - skelW / 2, eaDragStart.mx + dx));
      my = Math.max(skelH / 2, Math.min(canvasH - skelH / 2, eaDragStart.my + dy));
    } else {
      // Resize par le coin SE : échelle proportionnelle à la largeur.
      const newW = Math.max(152, eaDragStart.echelle * 380 + dx);
      mechelle = Math.max(0.4, Math.min(3, newW / 380));
      // Re-borner le centre avec la nouvelle taille.
      mx = Math.max(skelW / 2, Math.min(canvasW - skelW / 2, mx));
      my = Math.max(skelH / 2, Math.min(canvasH - skelH / 2, my));
    }
  }

  async function eaOnPointerUp() {
    if (!eaDragMode) return;
    eaDragMode = null;
    await commitConfig({
      minuteur_x: Math.round(mx),
      minuteur_y: Math.round(my),
      minuteur_echelle: Math.round(mechelle * 100) / 100,
    });
  }

  function eaCentrer() {
    mx = Math.round(canvasW / 2);
    my = Math.round(canvasH / 2);
    void commitConfig({ minuteur_x: mx, minuteur_y: my });
  }

  /// Formate des secondes → "M:SS" ou "H:MM:SS" (affichage dashboard).
  function formatTemps(total: number): string {
    const t = Math.max(0, Math.floor(total));
    const h = Math.floor(t / 3600);
    const m = Math.floor((t % 3600) / 60);
    const s = t % 60;
    if (h > 0) {
      return `${h}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
    }
    return `${m}:${String(s).padStart(2, "0")}`;
  }

  async function refreshSourcesAudio() {
    sourcesErreur = null;
    try {
      sourcesAudio = await listerSourcesAudioEcranAttente();
    } catch (e) {
      sourcesAudio = [];
      sourcesErreur = String(e);
    }
  }

  async function onImporter() {
    enCours = true;
    ecranAttenteErreur.set(null);
    try {
      await importerMediaEcranAttente();
    } catch (e) {
      ecranAttenteErreur.set(String(e));
    } finally {
      enCours = false;
    }
  }

  async function onRetirerMedia() {
    enCours = true;
    try {
      await retirerMediaEcranAttente();
      ecranAttenteErreur.set(null);
    } catch (e) {
      ecranAttenteErreur.set(String(e));
    } finally {
      enCours = false;
    }
  }

  async function onLancer() {
    enCours = true;
    ecranAttenteErreur.set(null);
    try {
      await lancerEcranAttente();
    } catch (e) {
      ecranAttenteErreur.set(String(e));
    } finally {
      enCours = false;
    }
  }

  async function onPauseToggle() {
    enCours = true;
    try {
      await togglePauseEcranAttente();
      ecranAttenteErreur.set(null);
    } catch (e) {
      ecranAttenteErreur.set(String(e));
    } finally {
      enCours = false;
    }
  }

  async function onArreter() {
    enCours = true;
    try {
      await arreterEcranAttente();
      ecranAttenteErreur.set(null);
    } catch (e) {
      ecranAttenteErreur.set(String(e));
    } finally {
      enCours = false;
    }
  }

  function onFermer() {
    ecranAttenteModalOpen.set(false);
  }
</script>

{#if $ecranAttenteModalOpen}
  <Modal
    title="Écran d'attente"
    onClose={onFermer}
    maxWidth="560px"
    hint="Affiché uniquement sur la diffusion OBS (écran « on revient ») — rien n'est rendu dans le dashboard. Le cadre et les couleurs reprennent ceux de la scène en cours."
  >
    <div class="ea-modal-body">
      <!-- État courant -->
      {#if actif}
        <div class="ea-statut">
          <span class="ea-pastille" class:pause={enPause}></span>
          <span class="ea-statut-texte">
            {enPause ? "En pause" : "Écran actif"} — {formatTemps(etat?.temps_restant ?? 0)}
          </span>
        </div>
      {/if}

      <!-- Média -->
      <div class="modal-bloc">
        <div class="modal-bloc-titre">Média (diffusion uniquement)</div>
        {#if mediaNom}
          <div class="ea-media-ligne">
            <span class="ea-media-nom" title={mediaNom}>{mediaNom}</span>
            <span class="ea-media-kind">{mediaKind === "video" ? "Vidéo" : "Image"}</span>
          </div>
        {:else}
          <p class="ea-info">Aucun média — un dégradé aux couleurs du cadre sera affiché.</p>
        {/if}
        <div class="ea-boutons">
          <button class="modal-action" disabled={enCours} onclick={onImporter}>
            {mediaNom ? "Remplacer" : "Importer un média"}
          </button>
          {#if mediaNom}
            <button class="modal-action danger" disabled={enCours} onclick={onRetirerMedia}>
              Retirer
            </button>
          {/if}
        </div>
      </div>

      <!-- Durée -->
      <div class="modal-bloc">
        <div class="modal-bloc-titre">Durée du chrono</div>
        <div class="ea-duree-row">
          <input
            class="ea-num"
            type="number"
            min="0"
            max="999"
            bind:value={dureeMin}
            onchange={commitDuree}
          />
          <span class="ea-unite">min</span>
          <input
            class="ea-num"
            type="number"
            min="0"
            max="59"
            bind:value={dureeSec}
            onchange={commitDuree}
          />
          <span class="ea-unite">sec</span>
        </div>
      </div>

      <!-- Position du minuteur (mini-canvas drag/resize) -->
      <div class="modal-bloc">
        <div class="modal-bloc-titre">Position du minuteur</div>
        <div
          class="ea-mini-canvas"
          bind:clientWidth={miniW}
          onpointermove={eaOnPointerMove}
          onpointerup={eaOnPointerUp}
          role="presentation"
        >
          <div
            class="ea-skel"
            style="left: {(mx - skelW / 2) * miniScale}px; top: {(my - skelH / 2) * miniScale}px; width: {skelW * miniScale}px; height: {skelH * miniScale}px;"
            onpointerdown={eaOnRectDown}
            role="presentation"
          >
            <span class="ea-skel-label">Minuteur</span>
            <div
              class="ea-handle-se"
              onpointerdown={eaOnHandleDown}
              role="presentation"
            ></div>
          </div>
        </div>
        <div class="ea-pos-row">
          <p class="ea-info">
            Glisser pour déplacer · coin pour redimensionner · {Math.round(mx)}, {Math.round(my)} · ×{mechelle.toFixed(2)}
          </p>
          <button class="modal-action ea-btn-centrer" onclick={eaCentrer}>Centrer</button>
        </div>
      </div>

      <!-- Titre -->
      <div class="modal-bloc">
        <div class="modal-bloc-titre">Titre affiché</div>
        <input
          class="ea-input"
          type="text"
          maxlength="80"
          placeholder="Stream Operating System"
          bind:value={titre}
          onchange={() => commitConfig({ titre })}
        />
        <label class="ea-check">
          <input
            type="checkbox"
            bind:checked={afficherTitre}
            onchange={() => commitConfig({ afficher_titre: afficherTitre })}
          />
          Afficher le titre (dégradé aux couleurs du cadre)
        </label>
      </div>

      <!-- Son -->
      <div class="modal-bloc">
        <div class="modal-bloc-titre">Son</div>
        <label class="ea-check">
          <input
            type="checkbox"
            bind:checked={son}
            onchange={() => commitConfig({ son })}
          />
          Son activé (vidéo sur la diffusion)
        </label>
        <div class="ea-volume-row">
          <input
            class="modal-range"
            type="range"
            min="0"
            max="100"
            step="1"
            disabled={!son}
            bind:value={volume}
            onchange={() => commitConfig({ volume: volume / 100 })}
          />
          <span class="ea-volume-val">{volume}%</span>
        </div>
      </div>

      <!-- Source audio OBS (routage — optionnel) -->
      <div class="modal-bloc">
        <div class="modal-bloc-titre">Source audio OBS</div>
        <div class="ea-source-row">
          <select
            class="ea-select"
            bind:value={sourceAudio}
            onchange={() => commitConfig({ source_audio: sourceAudio || undefined })}
          >
            <option value="">Automatique (SOS Audio Écran d'attente)</option>
            {#each sourcesAudio as nom (nom)}
              <option value={nom}>{nom}</option>
            {/each}
          </select>
          <button
            class="modal-action ea-btn-refresh"
            title="Recharger la liste (connexion OBS one-shot)"
            onclick={() => void refreshSourcesAudio()}
          >↻</button>
        </div>
        {#if sourcesErreur}
          <p class="ea-info">Liste indisponible — OBS non connecté (le routage audio sera tenté au lancement).</p>
        {:else if $obsStatus !== "connected"}
          <p class="ea-info">OBS non connecté — le routage audio sera tenté avec les identifiants enregistrés.</p>
        {/if}
      </div>

      {#if $ecranAttenteErreur}
        <p class="ea-erreur">{$ecranAttenteErreur}</p>
      {/if}

      <!-- Contrôles -->
      <div class="modal-actions">
        {#if actif}
          <button class="modal-action" disabled={enCours} onclick={onPauseToggle}>
            {enPause ? "Reprendre" : "Pause"}
          </button>
          <button class="modal-action danger" disabled={enCours} onclick={onArreter}>
            Arrêter
          </button>
          <button class="modal-action primary" disabled={enCours} onclick={onLancer}>
            Relancer
          </button>
        {:else}
          <button class="modal-action primary" disabled={enCours} onclick={onLancer}>
            Lancer l'écran d'attente
          </button>
        {/if}
      </div>
    </div>
  </Modal>
{/if}

<style>
  .ea-modal-body {
    padding: 1rem;
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
    overflow-y: auto;
  }
  .ea-statut {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.5rem 0.7rem;
    background: var(--fond-controle);
    border: 1px solid var(--bordure-ui);
    border-radius: var(--rayon-petit);
  }
  .ea-pastille {
    width: 0.6rem;
    height: 0.6rem;
    border-radius: 50%;
    background: var(--message-success-color, #4caf50);
    box-shadow: 0 0 6px var(--message-success-color, #4caf50);
    flex-shrink: 0;
  }
  .ea-pastille.pause {
    background: var(--accent-ui);
    box-shadow: 0 0 6px var(--accent-ui);
  }
  .ea-statut-texte {
    font-size: 0.85rem;
    font-variant-numeric: tabular-nums;
  }
  .ea-media-ligne {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    margin-bottom: 0.45rem;
  }
  .ea-media-nom {
    flex: 1;
    min-width: 0;
    font-size: 0.85rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .ea-media-kind {
    font-size: 0.7rem;
    padding: 0.1rem 0.45rem;
    border: 1px solid var(--bordure-ui);
    border-radius: var(--rayon-petit);
    opacity: 0.75;
    flex-shrink: 0;
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }
  .ea-info {
    font-size: 0.78rem;
    opacity: 0.65;
    margin: 0.25rem 0;
    font-style: italic;
  }
  .ea-boutons {
    display: flex;
    gap: 0.4rem;
  }
  .ea-boutons .modal-action {
    flex: 1;
  }
  .ea-duree-row {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }
  .ea-num {
    width: 70px;
    padding: 0.4rem 0.5rem;
    background: var(--fond-controle);
    color: var(--texte);
    border: 1px solid var(--bordure-ui);
    font: inherit;
    font-size: 0.9rem;
    font-variant-numeric: tabular-nums;
  }
  .ea-num:focus {
    outline: none;
    border-color: var(--accent-ui);
  }
  .ea-unite {
    font-size: 0.8rem;
    opacity: 0.7;
  }
  .ea-input {
    width: 100%;
    padding: 0.5rem 0.7rem;
    background: var(--fond-controle);
    color: var(--texte);
    border: 1px solid var(--bordure-ui);
    font: inherit;
    font-size: 0.9rem;
    margin-bottom: 0.4rem;
  }
  .ea-input:focus {
    outline: none;
    border-color: var(--accent-ui);
  }
  .ea-check {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    font-size: 0.85rem;
    cursor: pointer;
    margin-bottom: 0.4rem;
  }
  .ea-check input {
    accent-color: var(--accent-ui);
    cursor: pointer;
  }
  .ea-volume-row {
    display: flex;
    align-items: center;
    gap: 0.6rem;
  }
  .ea-volume-row .modal-range {
    flex: 1;
  }
  .ea-volume-val {
    font-size: 0.8rem;
    opacity: 0.8;
    font-variant-numeric: tabular-nums;
    width: 40px;
    text-align: right;
  }
  .ea-source-row {
    display: flex;
    gap: 0.4rem;
  }
  .ea-select {
    flex: 1;
    min-width: 0;
    padding: 0.4rem 0.5rem;
    background: var(--fond-controle);
    color: var(--texte);
    border: 1px solid var(--bordure-ui);
    font: inherit;
    font-size: 0.85rem;
    cursor: pointer;
  }
  .ea-select:focus {
    outline: none;
    border-color: var(--accent-ui);
  }
  .ea-select option {
    background: var(--fond);
    color: var(--texte);
  }
  .ea-btn-refresh {
    flex: 0 0 auto;
    padding: 0.3rem 0.6rem;
  }
  /* Mini-canvas de positionnement du minuteur (même mécanique que le
     squelette de position d'InteractionViewerModal). */
  .ea-mini-canvas {
    position: relative;
    width: 100%;
    aspect-ratio: 16 / 9;
    background: var(--fond);
    border: 1px solid var(--bordure-ui);
    overflow: hidden;
    touch-action: none;
    user-select: none;
    margin-bottom: 0.4rem;
  }
  .ea-skel {
    position: absolute;
    background: color-mix(in srgb, var(--accent-ui) 12%, transparent);
    border: 2px solid var(--bordure-ui-active);
    border-radius: 8px;
    cursor: move;
    display: flex;
    align-items: center;
    justify-content: center;
    touch-action: none;
  }
  .ea-skel:hover {
    background: color-mix(in srgb, var(--accent-ui) 18%, transparent);
  }
  .ea-skel-label {
    font-size: 0.7rem;
    opacity: 0.6;
    pointer-events: none;
  }
  .ea-handle-se {
    position: absolute;
    bottom: -6px;
    right: -6px;
    width: 10px;
    height: 10px;
    background: var(--accent-ui);
    border: 1px solid var(--fond);
    cursor: nwse-resize;
  }
  .ea-pos-row {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }
  .ea-pos-row .ea-info {
    flex: 1;
    margin: 0;
    font-variant-numeric: tabular-nums;
  }
  .ea-btn-centrer {
    flex: 0 0 auto;
    padding: 0.25rem 0.7rem;
    font-size: 0.78rem;
  }
  .ea-erreur {
    font-size: 0.8rem;
    color: var(--message-error-color);
    margin: 0;
    padding: 0.4rem 0.6rem;
    border: 1px solid var(--message-error-color);
    border-radius: var(--rayon-petit);
  }
  .modal-actions {
    justify-content: flex-end;
  }
</style>
