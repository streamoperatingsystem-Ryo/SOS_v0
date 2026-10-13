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
  import { t } from "../stores/langue";
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
  let layout = $state("signature");

  // Position/échelle des éléments de l'overlay (centre x/y en px canvas +
  // échelle) — minuteur, Streamy et wordmark logo. Éditées via le mini-canvas
  // ci-dessous (drag = déplacer, coin = resize) — même mécanique que le
  // « Squelette de position » d'InteractionViewerModal.
  type EaElem = "minuteur" | "streamy" | "logo";
  interface EaPos {
    x: number;
    y: number;
    e: number;
  }
  let eaPos = $state<Record<EaElem, EaPos>>({
    minuteur: { x: 960, y: 540, e: 1 },
    streamy: { x: 170, y: 180, e: 0.6 },
    logo: { x: 960, y: 140, e: 1 },
  });

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
    layout = c.layout;
    eaPos = {
      minuteur: { x: c.minuteur_x, y: c.minuteur_y, e: c.minuteur_echelle },
      streamy: { x: c.streamy_x, y: c.streamy_y, e: c.streamy_echelle },
      logo: { x: c.logo_x, y: c.logo_y, e: c.logo_echelle },
    };
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

  // ===== Mini-canvas : drag/resize des éléments de l'overlay =====
  // Canvas de référence = la scène courante (fallback 1920×1080). Chaque
  // squelette représente un élément (repère px à échelle 1, couleur
  // distincte) ; x/y stockés = CENTRE de l'élément (convention diffusion).
  let miniW = $state(300);
  let canvasW = $derived($sceneStore.canvasW ?? 1920);
  let canvasH = $derived($sceneStore.canvasH ?? 1080);
  let miniScale = $derived(miniW / canvasW);

  // Repères px à échelle 1 + bornes d'échelle + couleur par élément.
  const EA_SKELS: {
    id: EaElem;
    baseW: number;
    baseH: number;
    minE: number;
    maxE: number;
    color: string;
  }[] = [
    { id: "minuteur", baseW: 380, baseH: 160, minE: 0.4, maxE: 3, color: "var(--accent-ui)" },
    { id: "streamy", baseW: 300, baseH: 430, minE: 0.3, maxE: 2, color: "#22d3ee" },
    { id: "logo", baseW: 480, baseH: 262, minE: 0.3, maxE: 2, color: "#ec4899" },
  ];

  function eaSkelDef(id: EaElem) {
    return EA_SKELS.find((s) => s.id === id)!;
  }

  type EaDragMode = "move" | "resize" | null;
  let eaDragElem = $state<EaElem | null>(null);
  let eaDragMode = $state<EaDragMode>(null);
  let eaDragStart = { px: 0, py: 0, x: 0, y: 0, e: 1 };

  function eaOnRectDown(e: PointerEvent, id: EaElem) {
    e.stopPropagation();
    e.preventDefault();
    eaDragElem = id;
    eaDragMode = "move";
    eaDragStart = { px: e.clientX, py: e.clientY, ...eaPos[id] };
    (e.target as HTMLElement).setPointerCapture(e.pointerId);
  }

  function eaOnHandleDown(e: PointerEvent, id: EaElem) {
    e.stopPropagation();
    e.preventDefault();
    eaDragElem = id;
    eaDragMode = "resize";
    eaDragStart = { px: e.clientX, py: e.clientY, ...eaPos[id] };
    (e.target as HTMLElement).setPointerCapture(e.pointerId);
  }

  function eaOnPointerMove(e: PointerEvent) {
    if (!eaDragMode || !eaDragElem) return;
    const sk = eaSkelDef(eaDragElem);
    const pos = eaPos[eaDragElem];
    const dx = (e.clientX - eaDragStart.px) / miniScale;
    const dy = (e.clientY - eaDragStart.py) / miniScale;

    if (eaDragMode === "move") {
      // Centre borné pour que l'élément reste entièrement dans le canvas.
      pos.x = Math.max((sk.baseW * pos.e) / 2, Math.min(canvasW - (sk.baseW * pos.e) / 2, eaDragStart.x + dx));
      pos.y = Math.max((sk.baseH * pos.e) / 2, Math.min(canvasH - (sk.baseH * pos.e) / 2, eaDragStart.y + dy));
    } else {
      // Resize par le coin SE : échelle proportionnelle à la largeur.
      const newW = Math.max(60, eaDragStart.e * sk.baseW + dx);
      pos.e = Math.max(sk.minE, Math.min(sk.maxE, newW / sk.baseW));
      // Re-borner le centre avec la nouvelle taille.
      pos.x = Math.max((sk.baseW * pos.e) / 2, Math.min(canvasW - (sk.baseW * pos.e) / 2, pos.x));
      pos.y = Math.max((sk.baseH * pos.e) / 2, Math.min(canvasH - (sk.baseH * pos.e) / 2, pos.y));
    }
  }

  async function eaOnPointerUp() {
    if (!eaDragMode || !eaDragElem) return;
    const id = eaDragElem;
    eaDragMode = null;
    eaDragElem = null;
    const p = eaPos[id];
    const rx = Math.round(p.x);
    const ry = Math.round(p.y);
    const re = Math.round(p.e * 100) / 100;
    if (id === "minuteur") {
      await commitConfig({ minuteur_x: rx, minuteur_y: ry, minuteur_echelle: re });
    } else if (id === "streamy") {
      await commitConfig({ streamy_x: rx, streamy_y: ry, streamy_echelle: re });
    } else {
      await commitConfig({ logo_x: rx, logo_y: ry, logo_echelle: re });
    }
  }

  /// Réinitialise les positions aux valeurs par défaut (minuteur centré,
  /// Streamy haut-gauche, logo haut-centre).
  function eaResetPos() {
    eaPos = {
      minuteur: { x: Math.round(canvasW / 2), y: Math.round(canvasH / 2), e: 1 },
      streamy: { x: 170, y: 180, e: 0.6 },
      logo: { x: Math.round(canvasW / 2), y: 140, e: 1 },
    };
    void commitConfig({
      minuteur_x: eaPos.minuteur.x,
      minuteur_y: eaPos.minuteur.y,
      minuteur_echelle: 1,
      streamy_x: 170,
      streamy_y: 180,
      streamy_echelle: 0.6,
      logo_x: eaPos.logo.x,
      logo_y: 140,
      logo_echelle: 1,
    });
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
    title={$t("ecranAttenteModal.titre")}
    onClose={onFermer}
    crochet="var(--crochet-cyan)"
    maxWidth="35rem"
    hint={$t("ecranAttenteModal.hint")}
  >
    <div class="ea-modal-body">
      <!-- État courant -->
      {#if actif}
        <div class="ea-statut">
          <span class="ea-pastille" class:pause={enPause}></span>
          <span class="ea-statut-texte">
            {enPause ? $t("ecranAttenteModal.enPause") : $t("ecranAttenteModal.ecranActif")} — {formatTemps(etat?.temps_restant ?? 0)}
          </span>
        </div>
      {/if}

      <!-- Média -->
      <div class="modal-bloc">
        <div class="modal-bloc-titre">{$t("ecranAttenteModal.mediaTitre")}</div>
        {#if mediaNom}
          <div class="ea-media-ligne">
            <span class="ea-media-nom" title={mediaNom}>{mediaNom}</span>
            <span class="ea-media-kind">{mediaKind === "video" ? $t("commun.video") : $t("commun.image")}</span>
          </div>
        {:else}
          <p class="ea-info">{$t("ecranAttenteModal.aucunMedia")}</p>
        {/if}
        <div class="ea-boutons">
          <button class="action" disabled={enCours} onclick={onImporter}>
            {mediaNom ? $t("ecranAttenteModal.remplacer") : $t("ecranAttenteModal.importerMedia")}
          </button>
          {#if mediaNom}
            <button class="action danger" disabled={enCours} onclick={onRetirerMedia}>
              {$t("commun.retirer")}
            </button>
          {/if}
        </div>
      </div>

      <!-- Style d'écran (signature = marque StreamOS / minimal = historique) -->
      <div class="modal-bloc">
        <div class="modal-bloc-titre">{$t("ecranAttenteModal.style")}</div>
        <select
          class="ea-select"
          bind:value={layout}
          onchange={() => commitConfig({ layout })}
        >
          <option value="signature">{$t("ecranAttenteModal.styleSignature")}</option>
          <option value="minimal">{$t("ecranAttenteModal.styleMinimal")}</option>
        </select>
      </div>

      <!-- Durée -->
      <div class="modal-bloc">
        <div class="modal-bloc-titre">{$t("ecranAttenteModal.duree")}</div>
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
        <div class="modal-bloc-titre">{$t("ecranAttenteModal.position")}</div>
        <div
          class="ea-mini-canvas"
          bind:clientWidth={miniW}
          onpointermove={eaOnPointerMove}
          onpointerup={eaOnPointerUp}
          role="presentation"
        >
          {#each EA_SKELS as sk (sk.id)}
            {@const p = eaPos[sk.id]}
            {@const w = sk.baseW * p.e}
            {@const h = sk.baseH * p.e}
            <div
              class="ea-skel"
              style="left: {(p.x - w / 2) * miniScale}px; top: {(p.y - h / 2) * miniScale}px; width: {w * miniScale}px; height: {h * miniScale}px; border-color: {sk.color};"
              onpointerdown={(e) => eaOnRectDown(e, sk.id)}
              role="presentation"
            >
              <span class="ea-skel-label">
                {sk.id === "minuteur"
                  ? $t("ecranAttenteModal.minuteur")
                  : sk.id === "streamy"
                    ? "Streamy"
                    : "Logo"}
              </span>
              <div
                class="ea-handle-se"
                onpointerdown={(e) => eaOnHandleDown(e, sk.id)}
                role="presentation"
              ></div>
            </div>
          {/each}
        </div>
        <div class="ea-pos-row">
          <p class="ea-info">
            {$t("ecranAttenteModal.glisserInfo")}
          </p>
          <button class="action ea-btn-centrer" onclick={eaResetPos}>{$t("ecranAttenteModal.resetPos")}</button>
        </div>
      </div>

      <!-- Titre -->
      <div class="modal-bloc">
        <div class="modal-bloc-titre">{$t("ecranAttenteModal.titreAffiche")}</div>
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
          {$t("ecranAttenteModal.afficherTitre")}
        </label>
      </div>

      <!-- Son -->
      <div class="modal-bloc">
        <div class="modal-bloc-titre">{$t("ecranAttenteModal.son")}</div>
        <label class="ea-check">
          <input
            type="checkbox"
            bind:checked={son}
            onchange={() => commitConfig({ son })}
          />
          {$t("ecranAttenteModal.sonActif")}
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
        <div class="modal-bloc-titre">{$t("ecranAttenteModal.sourceAudio")}</div>
        <div class="ea-source-row">
          <select
            class="ea-select"
            bind:value={sourceAudio}
            onchange={() => commitConfig({ source_audio: sourceAudio || undefined })}
          >
            <option value="">{$t("ecranAttenteModal.sourceAuto")}</option>
            {#each sourcesAudio as nom (nom)}
              <option value={nom}>{nom}</option>
            {/each}
          </select>
          <button
            class="action ea-btn-refresh"
            title={$t("ecranAttenteModal.rechargerSources")}
            onclick={() => void refreshSourcesAudio()}
          >↻</button>
        </div>
        {#if sourcesErreur}
          <p class="ea-info">{$t("ecranAttenteModal.listeIndispo")}</p>
        {:else if $obsStatus !== "connected"}
          <p class="ea-info">{$t("ecranAttenteModal.obsNonConnecte")}</p>
        {/if}
      </div>

      {#if $ecranAttenteErreur}
        <p class="ea-erreur">{$ecranAttenteErreur}</p>
      {/if}

      <!-- Contrôles -->
      <div class="modal-actions">
        {#if actif}
          <button class="action" disabled={enCours} onclick={onPauseToggle}>
            {enPause ? $t("ecranAttenteModal.reprendre") : $t("commun.pause")}
          </button>
          <button class="action danger" disabled={enCours} onclick={onArreter}>
            {$t("commun.arreter")}
          </button>
          <button class="action primary" disabled={enCours} onclick={onLancer}>
            {$t("ecranAttenteModal.relancer")}
          </button>
        {:else}
          <button class="action primary" disabled={enCours} onclick={onLancer}>
            {$t("ecranAttenteModal.lancer")}
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
  .ea-boutons .action {
    flex: 1;
  }
  .ea-duree-row {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }
  .ea-num {
    width: 4.375rem;
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
    width: 2.5rem;
    text-align: right;
  }
  .ea-source-row {
    display: flex;
    gap: 0.4rem;
  }
  /* Style = global (app.css) — reste le layout flex de la rangée. */
  .ea-select {
    flex: 1;
    min-width: 0;
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
</style>
