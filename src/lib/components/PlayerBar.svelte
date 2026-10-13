<script lang="ts">
  // Barre lecteur dashboard : contrôleur de :4321, PAS du dashboard.
  // Écrit mediaPaused/mediaTime dans la scène (commitScene → snapshot WS →
  // :4321 applique play/pause/seek/loop). Le dashboard reste figé (vignette
  // à mediaTime, jamais de play()). Aucun play()/pause() sur videoEl ici.
  // videoEl sert uniquement à lire la duration (loadedmetadata).
  // Slider seek : oninput → onSeekLive (état local, la vignette suit) ;
  // onchange (release) + boutons ±5 s → onSeek (commit, 1 update_scene).
  import { t } from "../stores/langue";

  let {
    videoEl,
    mediaPaused,
    mediaTime,
    loopA,
    loopB,
    onTogglePlay,
    onSeek,
    onSeekLive,
    onLoopChange,
  }: {
    videoEl: HTMLVideoElement | undefined;
    mediaPaused: boolean;
    mediaTime: number;
    /// Bornes de la boucle A–B (secondes, null = non définie). Les deux
    /// définies avec B > A = boucle sur le segment en diffusion.
    loopA: number | null;
    loopB: number | null;
    onTogglePlay: () => void;
    onSeek: (time: number) => void;
    onSeekLive: (time: number) => void;
    onLoopChange: (a: number | null, b: number | null) => void;
  } = $props();

  let duration = $state(0);

  // Lecture de la duration depuis la vidéo dashboard (preload="metadata").
  $effect(() => {
    const v = videoEl;
    if (!v) {
      duration = 0;
      return;
    }
    duration = Number.isFinite(v.duration) ? v.duration : 0;
    const onMeta = () => {
      duration = Number.isFinite(v.duration) ? v.duration : 0;
    };
    v.addEventListener("loadedmetadata", onMeta);
    v.addEventListener("durationchange", onMeta);
    return () => {
      v.removeEventListener("loadedmetadata", onMeta);
      v.removeEventListener("durationchange", onMeta);
    };
  });

  // Format m:ss.
  function fmt(t: number): string {
    if (!Number.isFinite(t) || t < 0) t = 0;
    const m = Math.floor(t / 60);
    const s = Math.floor(t % 60);
    return m + ":" + (s < 10 ? "0" : "") + s;
  }

  function onMinus5() {
    onSeek(Math.max(0, mediaTime - 5));
  }

  function onPlus5() {
    onSeek(Math.min(duration || mediaTime, mediaTime + 5));
  }

  // Drag : preview local (vignette + thumb suivent via mediaTime), zéro commit.
  function onSeekInput(e: Event) {
    const t = parseFloat((e.target as HTMLInputElement).value);
    if (Number.isFinite(t)) onSeekLive(t);
  }

  // Release (change) : UN commit → snapshot :4321 (seek effectif en diffusion).
  function onSeekChange(e: Event) {
    const t = parseFloat((e.target as HTMLInputElement).value);
    if (Number.isFinite(t)) onSeek(t);
  }

  // Marque A ou B à la position courante du slider. Les bornes sont
  // réordonnées (min, max) : l'ordre de marquage importe peu, A < B toujours.
  function onMarqueA() {
    if (loopB === null) onLoopChange(mediaTime, null);
    else onLoopChange(Math.min(mediaTime, loopB), Math.max(mediaTime, loopB));
  }
  function onMarqueB() {
    if (loopA === null) onLoopChange(null, mediaTime);
    else onLoopChange(Math.min(mediaTime, loopA), Math.max(mediaTime, loopA));
  }
  function onEffaceLoop() {
    onLoopChange(null, null);
  }
</script>

<div class="player">
  <div class="player-row">
    <button class="action small centre" onclick={onMinus5}>−5 s</button>
    <button class="action small centre" onclick={onTogglePlay}>
      {mediaPaused ? $t("commun.lecture") : $t("commun.pause")}
    </button>
    <button class="action small centre" onclick={onPlus5}>+5 s</button>
  </div>
  <input
    class="range"
    type="range"
    min="0"
    max={duration || 0}
    step="0.1"
    value={mediaTime}
    oninput={onSeekInput}
    onchange={onSeekChange}
  />
  <span class="player-time">{fmt(mediaTime)} / {fmt(duration)}</span>
  <div class="player-row">
    <button class="action small centre" class:actif={loopA !== null} onclick={onMarqueA}>A</button>
    <button class="action small centre" class:actif={loopB !== null} onclick={onMarqueB}>B</button>
    <button class="action small centre" onclick={onEffaceLoop}>{$t("carteEdition.boucleEffacer")}</button>
  </div>
  <span class="player-time">
    {#if loopA !== null && loopB !== null}
      {$t("carteEdition.boucleEtat", { a: fmt(loopA), b: fmt(loopB) })}
    {:else}
      {$t("carteEdition.boucleHint")}
    {/if}
  </span>
</div>

<style>
  .player {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
    /* Boutons = recette « glace » commune (fill neutre translucide). La
       couleur de la carte d'édition vit sur sa bordure, pas dans le fill. */
  }
  .player-row {
    display: grid;
    grid-template-columns: 1fr 1fr 1fr;
    gap: 0.25rem;
  }
  .player-time {
    font-size: 0.75rem;
    opacity: 0.8;
    font-variant-numeric: tabular-nums;
    text-align: center;
  }
  .range {
    width: 100%;
    accent-color: var(--accent-ui);
    background: var(--fond);
    color: var(--texte);
  }
</style>
