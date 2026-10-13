<script lang="ts">
  // Onglet « Musique » de la modale « Interactions Live » — demandes de
  // musique par les viewers (!rs <titre>) sur la bibliothèque LOCALE du
  // streamer (medias/musique/), jouée côté diffusion (browser source OBS).
  //
  // L'état (config + en cours + file + bibliothèque) vit côté Rust et arrive
  // par l'event "musique:update" — le store musiqueEtat est la seule source
  // de vérité affichée. Les réglages sont édités dans une copie locale
  // (cfg) et sauvegardés en un clic (musique_set_config, noms normalisés
  // côté Rust). Le volume est LUI appliqué à chaud (musique_volume).
  import { onMount } from "svelte";
  import { get } from "svelte/store";
  import { t } from "../stores/langue";
  import { ecouterSon, stopperEcoute } from "../stores/presentation";
  import {
    musiqueEtat,
    initMusique,
    sauverConfigMusique,
    importerMusique,
    supprimerFichierMusique,
    testerMusique,
  } from "../stores/musique";
  import LecteurMusique from "./LecteurMusique.svelte";
  import type { MusiqueConfig, MusiqueFichier } from "../tauri";

  let etat = $derived($musiqueEtat);
  let biblio = $derived(etat?.bibliotheque ?? []);

  /// Copie locale éditable de la config (sauvegarde explicite — un clic).
  /// Initialisée une fois l'état chargé ; NE PAS la resync sur chaque
  /// musique:update (ça écraserait les champs en cours d'édition).
  let cfg = $state<MusiqueConfig | null>(null);
  /// Confirmation inline pour les actions destructives (suppr fichier, stop).
  let confirm = $state<{ texte: string; action: () => Promise<void> } | null>(
    null
  );
  let msg = $state<{ texte: string; ok: boolean } | null>(null);
  /// Src du fichier écouté localement (bouton Écouter → devient Stop).
  let ecouteEnCours = $state<string | null>(null);

  onMount(async () => {
    await initMusique();
    if (!cfg) cfg = get(musiqueEtat)?.config ?? null;
  });

  function demanderConfirm(texte: string, action: () => Promise<void>): void {
    confirm = { texte, action };
  }

  async function onConfirmer(): Promise<void> {
    const a = confirm?.action;
    confirm = null;
    if (a) await a();
  }

  /// Sauvegarde des réglages : le volume courant du store est préservé
  /// (le slider du bloc lecture applique à chaud et persiste déjà).
  async function onEnregistrer(): Promise<void> {
    if (!cfg) return;
    const vol = get(musiqueEtat)?.config.volume;
    if (typeof vol === "number") cfg = { ...cfg, volume: vol };
    const ok = await sauverConfigMusique(cfg);
    msg = {
      texte: get(t)(ok ? "interactions.musique.sauveOk" : "interactions.musique.sauveKo"),
      ok,
    };
  }

  async function onToggleActif(): Promise<void> {
    if (!cfg) return;
    cfg = { ...cfg, actif: !cfg.actif };
    await sauverConfigMusique(cfg);
  }

  function onEcouter(f: MusiqueFichier): void {
    if (ecouteEnCours === f.src) {
      stopperEcoute();
      ecouteEnCours = null;
      return;
    }
    ecouterSon(f.src);
    ecouteEnCours = f.src;
  }

  function tailleLisible(octets: number): string {
    if (octets >= 1024 * 1024) return (octets / 1048576).toFixed(1) + " Mo";
    return Math.max(1, Math.round(octets / 1024)) + " Ko";
  }

  function nomFichier(src: string): string {
    return src.split("/").pop() ?? src;
  }
</script>

<div class="musique-layout">
  <!-- Encart explicatif : le catalogue est 100 % LOCAL, rien n'est cherché
       en ligne — le nom du fichier EST le titre demandable. -->
  <div class="modal-bloc bloc-info">
    <div class="modal-bloc-titre">{$t("interactions.musique.titreInfo")}</div>
    <p class="hint">{$t("interactions.musique.intro")}</p>
    <p class="hint">{$t("interactions.musique.introCommandes")}</p>
    <p class="hint">{$t("interactions.musique.introNommage")}</p>
  </div>

  {#if !etat}
    <p class="hint-centre">…</p>
  {:else}
    <!-- Lecture courante + contrôles live : lecteur Winamp complet
         (playlist intégrée — la file vit dans le deck). -->
    <div class="modal-bloc">
      <div class="modal-bloc-titre">
        {$t("interactions.musique.enCours")}
        {#if etat.config.actif}
          <span class="etat on">ON</span>
        {:else}
          <span class="etat off">OFF</span>
        {/if}
      </div>
      <LecteurMusique {etat} />
    </div>

    <!-- Bibliothèque locale -->
    <div class="modal-bloc">
      <div class="modal-bloc-titre">
        {$t("interactions.musique.bibliotheque")} ({biblio.length})
        <span class="spacer"></span>
        <button class="action small" onclick={() => void importerMusique()}>
          + {$t("interactions.musique.importer")}
        </button>
      </div>
      {#if biblio.length === 0}
        <p class="hint">{$t("interactions.musique.biblioVide")}</p>
      {:else}
        <ul class="liste">
          {#each biblio as f (f.src)}
            <li class="ligne">
              <span class="ligne-titre">{f.titre}</span>
              <span class="ligne-meta">{tailleLisible(f.taille)}</span>
              <span class="ligne-actions">
                <button
                  class="action small"
                  title={$t("interactions.musique.ecouter")}
                  onclick={() => onEcouter(f)}
                  >{ecouteEnCours === f.src ? "⏹" : "👂"}</button
                >
                <button
                  class="action small"
                  title={$t("interactions.musique.tester")}
                  onclick={() => void testerMusique(f.src)}>▶</button
                >
                <button
                  class="action small danger"
                  title={$t("interactions.musique.supprimer")}
                  onclick={() =>
                    demanderConfirm(
                      get(t)("interactions.musique.confirmSuppr", {
                        titre: f.titre,
                      }),
                      async () => {
                        await supprimerFichierMusique(nomFichier(f.src));
                      }
                    )}>✕</button
                >
              </span>
            </li>
          {/each}
        </ul>
      {/if}
    </div>

    <!-- Réglages -->
    {#if cfg}
      <div class="modal-bloc">
        <div class="modal-bloc-titre">{$t("interactions.musique.reglages")}</div>
        <label class="row">
          <input
            type="checkbox"
            checked={cfg.actif}
            onchange={() => void onToggleActif()}
          />
          {$t("interactions.musique.actif")}
        </label>
        <div class="grid-cmds">
          <label class="field">
            <span class="field-label">{$t("interactions.musique.commandeRs")}</span>
            <span class="cmd">!<input type="text" bind:value={cfg.commande} /></span>
          </label>
          <label class="field">
            <span class="field-label">{$t("interactions.musique.commandeSkip")}</span>
            <span class="cmd">!<input type="text" bind:value={cfg.commande_skip} /></span>
          </label>
          <label class="field">
            <span class="field-label">{$t("interactions.musique.commandeRetirer")}</span>
            <span class="cmd">!<input type="text" bind:value={cfg.commande_retirer} /></span>
          </label>
          <label class="field">
            <span class="field-label">{$t("interactions.musique.commandeFile")}</span>
            <span class="cmd">!<input type="text" bind:value={cfg.commande_file} /></span>
          </label>
        </div>
        <div class="grid-cmds">
          <label class="field">
            <span class="field-label">{$t("interactions.musique.cooldown")}</span>
            <input type="number" min="0" bind:value={cfg.cooldown_viewer_s} />
          </label>
          <label class="field">
            <span class="field-label">{$t("interactions.musique.maxParViewer")}</span>
            <input type="number" min="0" bind:value={cfg.max_par_viewer} />
          </label>
          <label class="field">
            <span class="field-label">{$t("interactions.musique.fileMax")}</span>
            <input type="number" min="1" bind:value={cfg.file_max} />
          </label>
        </div>
        <label class="row">
          <input type="checkbox" bind:checked={cfg.reponses_chat} />
          {$t("interactions.musique.reponsesChat")}
        </label>
        <div>
          <button class="action" onclick={() => void onEnregistrer()}>
            {$t("interactions.musique.enregistrer")}
          </button>
        </div>
        <p class="hint">{$t("interactions.musique.hintModeration")}</p>
      </div>
    {/if}
  {/if}

  <!-- Message de retour + confirmation inline -->
  {#if msg}
    <p class="retour" class:ko={!msg.ok}>{msg.texte}</p>
  {/if}
  {#if confirm}
    <div class="confirm">
      <span>{confirm.texte}</span>
      <button class="action small danger" onclick={() => void onConfirmer()}>
        {$t("interactions.musique.confirmer")}
      </button>
      <button class="action small" onclick={() => (confirm = null)}>
        {$t("interactions.musique.annuler")}
      </button>
    </div>
  {/if}
</div>

<style>
  .musique-layout {
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
  }

  .bloc-info {
    border-color: color-mix(in srgb, var(--accent-ui) 40%, var(--bordure-ui));
  }
  .modal-bloc-titre {
    display: flex;
    align-items: center;
    gap: 0.4rem;
  }
  .spacer {
    flex: 1;
  }
  .etat {
    font-size: 0.65rem;
    font-weight: 700;
    padding: 0 0.35rem;
    border-radius: 0.25rem;
  }
  .etat.on {
    background: color-mix(in srgb, var(--accent-ui) 30%, transparent);
  }
  .etat.off {
    background: color-mix(in srgb, var(--message-error-color) 25%, transparent);
    color: var(--message-error-color);
  }

  .liste {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    max-height: 13.75rem;
    overflow-y: auto;
  }
  .ligne {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.25rem 0.4rem;
    border-radius: var(--rayon-petit);
    background: color-mix(in srgb, var(--fond-controle) 60%, transparent);
  }
  .ligne-titre {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 0.85rem;
  }
  .ligne-meta {
    font-size: 0.72rem;
    opacity: 0.55;
    white-space: nowrap;
  }
  .ligne-actions {
    display: flex;
    gap: 0.2rem;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    font-size: 0.85rem;
  }
  .grid-cmds {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(8.75rem, 1fr));
    gap: 0.5rem;
  }
  .cmd {
    display: flex;
    align-items: center;
    gap: 0.15rem;
    font-size: 0.85rem;
  }
  .hint-centre {
    font-size: 0.9rem;
    opacity: 0.6;
    text-align: center;
    margin: 2rem 0;
  }
  .retour {
    font-size: 0.8rem;
    margin: 0;
    color: var(--accent-ui);
  }
  .retour.ko {
    color: var(--message-error-color);
  }
  .confirm {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    font-size: 0.8rem;
    padding: 0.4rem 0.6rem;
    border: 1px solid color-mix(in srgb, var(--message-error-color) 40%, var(--bordure-ui));
    border-radius: var(--rayon-petit);
    /* Reste visible en bas du scroll — une confirmation déclenchée depuis
       un bouton haut de page doit rester à l'écran. */
    position: sticky;
    bottom: 0;
    background: var(--fond-panneau);
  }
</style>
