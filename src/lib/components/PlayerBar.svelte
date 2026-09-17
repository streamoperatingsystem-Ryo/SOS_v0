<script lang="ts">
  // Barre lecteur dashboard : contrôleur de :4321, PAS du dashboard.
  // Écrit mediaPaused/mediaTime dans la scène (commitScene → snapshot WS →
  // :4321 applique play/pause/seek/loop). Le dashboard reste figé (vignette
  // à mediaTime, jamais de play()). Aucun play()/pause() sur videoEl ici.
  // videoEl sert uniquement à lire la duration (loadedmetadata).
  let {
    videoEl,
    mediaPaused,
    mediaTime,
    onTogglePlay,
    onSeek,
  }: {
    videoEl: HTMLVideoElement | undefined;
    mediaPaused: boolean;
    mediaTime: number;
    onTogglePlay: () => void;
    onSeek: (time: number) => void;
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

  function onSeekInput(e: Event) {
    const t = parseFloat((e.target as HTMLInputElement).value);
    if (Number.isFinite(t)) onSeek(t);
  }
</script>

<div class="player">
  <div class="player-row">
    <button class="action" onclick={onMinus5}>−5 s</button>
    <button class="action" onclick={onTogglePlay}>
      {mediaPaused ? "Lecture" : "Pause"}
    </button>
    <button class="action" onclick={onPlus5}>+5 s</button>
  </div>
  <input
    class="range"
    type="range"
    min="0"
    max={duration || 0}
    step="0.1"
    value={mediaTime}
    oninput={onSeekInput}
  />
  <span class="player-time">{fmt(mediaTime)} / {fmt(duration)}</span>
</div>

<style>
  .player {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
    /* Teinte contextuelle héritée de la carte d'édition (--accent-carte).
       Si PlayerBar est rendu hors carte (fallback), --btn-tint = indigo dash. */
  }
  .player-row {
    display: grid;
    grid-template-columns: 1fr 1fr 1fr;
    gap: 0.25rem;
  }
  .action {
    /* Recette « verre Aero HUD » — teinte héritée du parent (carte d'édition). */
    background: var(--btn-surface, var(--fond-controle));
    box-shadow: var(--btn-inset, none);
    color: var(--texte);
    border: 1px solid var(--bordure);
    padding: 0.3rem 0.4rem;
    font: inherit;
    font-size: 0.8rem;
    cursor: pointer;
    text-align: center;
    transition: background 0.15s ease, box-shadow 0.15s ease, border-color 0.15s ease;
  }
  .action:hover {
    background: var(--btn-surface-hover, var(--fond-controle));
    box-shadow: var(--btn-inset-hover, none);
    border-color: var(--bordure-active);
  }
  .action:active {
    background: linear-gradient(180deg, var(--fond-controle) 0%, color-mix(in srgb, var(--btn-tint, var(--dash-accent)) 8%, var(--fond-controle)) 100%);
    box-shadow: inset 0 2px 4px rgba(0, 0, 0, 0.4);
  }
  .player-time {
    font-size: 0.75rem;
    opacity: 0.8;
    font-variant-numeric: tabular-nums;
    text-align: center;
  }
  .range {
    width: 100%;
    accent-color: var(--accent-violet);
    background: var(--fond);
    color: var(--texte);
  }
</style>
