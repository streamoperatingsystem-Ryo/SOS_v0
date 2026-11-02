<script lang="ts">
  // Mixeur audio natif — périphériques, gains, mute et vu-mètres.
  //
  // Source de vérité : `moteur.json` bloc `audio` (N sources, ordre du
  // mixage). Chaque changement est persisté ET appliqué à chaud : gain/mute
  // immédiats, périphérique/ajout/retrait = redémarrage de la seule source
  // concernée côté moteur.
  //
  // Les niveaux viennent du moteur (mesure pré-fader dans le thread de
  // capture + bus programme post-gain) et ne sont lus qu'ici : polling
  // 20 Hz démarré au montage, arrêté au démontage — aucun polling global.
  import { onMount } from "svelte";
  import {
    audioCfg,
    audioPeripheriques,
    audioEtat,
    audioErreur,
    chargerAudioCfg,
    chargerAudioPeripheriques,
    demarrerPollingAudio,
    arreterPollingAudio,
    sauverAudioCfg,
    sauverAudioCfgDiffere,
    resetEcretageAudio,
  } from "../stores/moteur";
  import { localeTag, t } from "../stores/langue";
  import type { AudioMoteurCfg, SourceAudioCfg } from "../tauri";
  import VuMetre from "./VuMetre.svelte";

  onMount(() => {
    void chargerAudioCfg();
    void chargerAudioPeripheriques();
    demarrerPollingAudio();
    return arreterPollingAudio;
  });

  let rafraichissement = $state(false);

  let formateurDb = $derived(
    new Intl.NumberFormat($localeTag, {
      minimumFractionDigits: 1,
      maximumFractionDigits: 1,
    })
  );

  /// Gain en millièmes → dB (borne d'affichage −60…+12).
  function dbDe(gain: number): number {
    if (gain <= 0) return -60;
    return Math.max(-60, Math.min(12, 20 * Math.log10(gain / 1000)));
  }

  /// dB → gain en millièmes (1…4000, jamais 0 : le mute est séparé).
  function gainDe(db: number): number {
    if (db <= -60) return 1;
    return Math.min(4000, Math.max(1, Math.round(1000 * Math.pow(10, db / 20))));
  }

  function fmtDb(db: number): string {
    return `${formateurDb.format(db)} dB`;
  }

  function libelleSource(source: SourceAudioCfg): string {
    const base = source.genre === "desktop" ? $t("audio.sonPc") : $t("audio.micro");
    const memeGenre = $audioCfg.sources.filter((s) => s.genre === source.genre);
    const rang = memeGenre.findIndex((s) => s.id === source.id) + 1;
    return rang > 1 ? `${base} ${rang}` : base;
  }

  function peripheriquesDe(genre: "desktop" | "micro") {
    return genre === "desktop" ? $audioPeripheriques.sorties : $audioPeripheriques.entrees;
  }

  function etatDe(id: string) {
    return $audioEtat?.sources.find((s) => s.id === id) ?? null;
  }

  function majSource(
    id: string,
    patch: Partial<SourceAudioCfg>,
    differe = false
  ): void {
    const cfg: AudioMoteurCfg = {
      sources: $audioCfg.sources.map((s) => (s.id === id ? { ...s, ...patch } : s)),
    };
    if (differe) sauverAudioCfgDiffere(cfg);
    else void sauverAudioCfg(cfg);
  }

  function ajouter(genre: "desktop" | "micro"): void {
    let id: string = genre;
    let rang = 2;
    while ($audioCfg.sources.some((s) => s.id === id)) id = `${genre}-${rang++}`;
    void sauverAudioCfg({
      sources: [
        ...$audioCfg.sources,
        { id, genre, peripherique: null, gain: 1000, mute: false },
      ],
    });
  }

  function supprimer(id: string): void {
    void sauverAudioCfg({
      sources: $audioCfg.sources.filter((s) => s.id !== id),
    });
  }

  function basculerMute(source: SourceAudioCfg): void {
    majSource(source.id, { mute: !source.mute });
  }

  async function rafraichir(): Promise<void> {
    rafraichissement = true;
    try {
      await chargerAudioPeripheriques();
    } finally {
      rafraichissement = false;
    }
  }
</script>

<div class="mixeur">
  <div class="mixeur-tete">
    <p class="hint">{$t("audio.hint")}</p>
    <button
      class="action small"
      onclick={rafraichir}
      disabled={rafraichissement}
      aria-label={$t("audio.rafraichir")}
    >
      {$t("audio.rafraichir")}
    </button>
  </div>

  {#if $audioErreur}
    <p class="hint msg-err">{$audioErreur}</p>
  {/if}

  {#if $audioCfg.sources.length === 0}
    <p class="hint msg-warn">{$t("audio.aucuneSource")}</p>
  {/if}

  <ul class="mixeur-liste">
    {#each $audioCfg.sources as source (source.id)}
      {@const libelle = libelleSource(source)}
      {@const etat = etatDe(source.id)}
      <li class="mixeur-source" role="group" aria-label={libelle}>
        <div class="mixeur-ligne">
          <span class="mixeur-nom">{libelle}</span>
          <button
            class="action small"
            class:actif={source.mute}
            aria-pressed={source.mute}
            aria-label={source.mute
              ? $t("audio.activer", { nom: libelle })
              : $t("audio.couper", { nom: libelle })}
            title={source.mute
              ? $t("audio.activer", { nom: libelle })
              : $t("audio.couper", { nom: libelle })}
            onclick={() => basculerMute(source)}
          >
            {$t("audio.mute")}
          </button>
          <button
            class="action small"
            aria-label={$t("audio.supprimer", { nom: libelle })}
            title={$t("audio.supprimer", { nom: libelle })}
            onclick={() => supprimer(source.id)}
          >
            ✕
          </button>
        </div>

        <div class="field">
          <span class="field-label">{$t("audio.peripherique")}</span>
          <select
            value={source.peripherique ?? ""}
            aria-label={$t("audio.peripheriqueDe", { nom: libelle })}
            onchange={(e) =>
              majSource(source.id, {
                peripherique: e.currentTarget.value === ""
                  ? null
                  : e.currentTarget.value,
              })}
          >
            <option value="">{$t("audio.defaut")}</option>
            {#each peripheriquesDe(source.genre) as p (p.id)}
              <option value={p.id}>{p.nom}{p.defaut ? " ✓" : ""}</option>
            {/each}
          </select>
        </div>

        <VuMetre niveau={etat?.niveau ?? null} label={$t("audio.niveauDe", { nom: libelle })} />

        <div class="mixeur-gain">
          <input
            class="modal-range"
            type="range"
            min="-60"
            max="12"
            step="0.5"
            value={dbDe(source.gain)}
            aria-label={$t("audio.gainDe", { nom: libelle })}
            aria-valuetext={fmtDb(dbDe(source.gain))}
            oninput={(e) =>
              majSource(source.id, { gain: gainDe(e.currentTarget.valueAsNumber) }, true)}
          />
          <span class="mixeur-db" aria-hidden="true">{fmtDb(dbDe(source.gain))}</span>
        </div>

        {#if etat?.erreur}
          <p class="hint msg-err">{etat.erreur}</p>
        {/if}
        {#if etat?.repli}
          <p class="hint msg-warn">
            {$t("audio.repli", { nom: etat.appareil ?? "?" })}
          </p>
        {/if}
      </li>
    {/each}
  </ul>

  <div class="mixeur-ajout">
    <button class="action small" onclick={() => ajouter("desktop")}>
      + {$t("audio.sonPc")}
    </button>
    <button class="action small" onclick={() => ajouter("micro")}>
      + {$t("audio.micro")}
    </button>
  </div>

  <div class="mixeur-master" role="group" aria-label={$t("audio.programme")}>
    <div class="mixeur-ligne">
      <span class="mixeur-nom">{$t("audio.programme")}</span>
      <button class="action small" onclick={() => void resetEcretageAudio()}>
        {$t("audio.resetEcretage")}
      </button>
    </div>
    <VuMetre niveau={$audioEtat?.master ?? null} label={$t("audio.programme")} />
    {#if !$audioEtat?.actif}
      <p class="hint">{$t("audio.inactif")}</p>
    {/if}
  </div>
</div>

<style>
  .mixeur {
    display: flex;
    flex-direction: column;
    gap: 0.55rem;
    min-width: 0;
  }
  .mixeur-tete {
    display: flex;
    align-items: flex-start;
    gap: 0.5rem;
    justify-content: space-between;
  }
  .mixeur-liste {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .mixeur-source,
  .mixeur-master {
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
    border: 1px solid var(--bordure-ui);
    border-radius: var(--rayon);
    padding: 0.45rem 0.55rem;
    background: var(--fond-panneau);
    min-width: 0;
  }
  .mixeur-ligne {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    min-width: 0;
  }
  .mixeur-nom {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 0.82rem;
    font-weight: 600;
  }
  .mixeur-gain {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }
  .mixeur-gain .modal-range {
    flex: 1;
    min-width: 0;
  }
  .mixeur-db {
    min-width: 4.2rem;
    text-align: right;
    font-size: 0.72rem;
    font-variant-numeric: tabular-nums;
    opacity: 0.85;
  }
  .mixeur-ajout {
    display: flex;
    gap: 0.4rem;
    flex-wrap: wrap;
  }
  .mixeur-master {
    border-color: var(--accent-ui);
  }
</style>
