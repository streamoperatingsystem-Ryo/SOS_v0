<script lang="ts">
  // Lecteur « Winamp rétro » de l'onglet Musique — deck visuel complet
  // façon Winamp 2.x : barre de titre dégradée (shade cliquable — tout
  // le deck se replie), grand compteur temps LED, écran LCD vert
  // phosphore (marquee + indicateurs kbps/kHz/stereo), clutterbar
  // O A I D V, équalizer 16 bandes avec peak dots, barre de progression
  // crantée, transport bevel, playlist intégrée (la file d'attente vit
  // ici — plus de bloc séparé dans l'onglet) et chips cooldowns.
  //
  // Le son reste sur la diffusion (browser source OBS) : ce composant est
  // une télécommande + visualisation. L'état arrive par "musique:update"
  // (store musiqueEtat) ; la progression est un SNAPSHOT figé à l'emit
  // ({position_ms, pausee, duree_s}) avancé ici en tick local 250 ms —
  // zéro trafic périodique. L'équalizer lit les bandes réelles poussées
  // par "musique:spectrum" (AnalyserNode de l'audio ombre côté diffusion,
  // ~8 Hz) et retombe sur une animation simulée si aucune frame n'arrive
  // pendant la lecture (PREVIEW, AudioContext suspendu).
  //
  // Les contrôles réutilisent les commandes existantes (aucune nouvelle) :
  // ⏮ = musiqueTester(src courant) → rejoue la piste à zéro ; ⏏ =
  // importerMusique() (l'eject ouvre l'import, comme Winamp ouvrait le
  // file picker). STOP vide la file → confirmation INLINE dans le deck
  // (bandeau sous le transport — pas de dépendance à l'onglet parent).
  import { onMount } from "svelte";
  import { get } from "svelte/store";
  import { t } from "../stores/langue";
  import {
    musiqueBands,
    musiqueBandsTs,
    musiqueSkip,
    musiquePause,
    musiqueStop,
    musiqueVider,
    musiqueRetirer,
    musiqueDeplacer,
    musiqueVolume,
    testerMusique,
    importerMusique,
  } from "../stores/musique";
  import type { MusiqueEtat } from "../tauri";

  let { etat }: { etat: MusiqueEtat } = $props();

  const NB_BANDES = 16;

  let enCours = $derived(etat.en_cours);
  let file = $derived(etat.file);
  let pausee = $derived(etat.progression?.pausee ?? false);

  /// Repli « shade » Winamp : la barre de titre seule reste visible.
  let shade = $state(false);

  /// Confirmation inline du ⏹ (stop + vide la file) — affichée sous le
  /// transport, à côté du bouton qui l'a déclenchée.
  let confirmStop = $state(false);

  // ── Progression ────────────────────────────────────────────────────
  // Snapshot figé à l'emit : {pos, pausee, dur, at}. Le tick local avance
  // pos + (now - at) tant que pausee=false ; resync à chaque musique:update.
  let snap = $state<{
    pos: number;
    pausee: boolean;
    dur: number | null;
    at: number;
  } | null>(null);

  $effect(() => {
    const p = etat.progression;
    snap = p
      ? { pos: p.position_ms, pausee: p.pausee, dur: p.duree_s, at: Date.now() }
      : null;
  });

  // Cooldowns : restant_s figé à l'emit, décrément local jusqu'au prochain
  // update. cdAt = réception du snapshot courant.
  let cdSnap = $state<{ restant: number; pseudo: string }[]>([]);
  let cdAt = $state(0);
  $effect(() => {
    cdSnap = (etat.cooldowns_actifs ?? []).map((c) => ({
      pseudo: c.pseudo,
      restant: c.restant_s,
    }));
    cdAt = Date.now();
  });

  // ── Valeurs affichées (rafraîchies par le tick 250 ms) ─────────────
  let posMs = $state(0);
  let cds = $state<{ pseudo: string; restant: number }[]>([]);
  // Bandes affichées : réelles si frames récentes, simulées sinon.
  let bandes = $state<number[]>(new Array(NB_BANDES).fill(0));
  /// Peak dots : crête par bande retombant doucement (signature Winamp).
  let pics = $state<number[]>(new Array(NB_BANDES).fill(0));

  let tick: ReturnType<typeof setInterval> | null = null;
  onMount(() => {
    tick = setInterval(tickUi, 250);
    return () => {
      if (tick) clearInterval(tick);
    };
  });

  function tickUi(): void {
    // Position.
    if (snap) {
      posMs = snap.pausee ? snap.pos : snap.pos + (Date.now() - snap.at);
      if (snap.dur != null) posMs = Math.min(posMs, snap.dur * 1000);
    } else {
      posMs = 0;
    }
    // Cooldowns décrémentés.
    const ecouleCd = (Date.now() - cdAt) / 1000;
    cds = cdSnap
      .map((c) => ({ pseudo: c.pseudo, restant: Math.ceil(c.restant - ecouleCd) }))
      .filter((c) => c.restant > 0);
    // Équalizer : frames réelles < 600 ms → bandes réelles ; sinon simulé
    // pendant la lecture, décroissance en pause/stop.
    const reel = Date.now() - get(musiqueBandsTs) < 600;
    const joue = !!enCours && !pausee;
    if (reel) {
      bandes = get(musiqueBands).slice(0, NB_BANDES);
    } else {
      const tt = Date.now() / 1000;
      bandes = bandes.map((b, i) => {
        const cible = joue
          ? 60 +
            140 *
              Math.abs(Math.sin(tt * (0.7 + i * 0.13) + i)) *
              (0.55 + 0.45 * Math.sin(tt * 2.3 + i * 2))
          : 0;
        return b + (cible - b) * 0.35;
      });
    }
    // Peak dots : accrochent la bande, retombent en ~1,5 s.
    pics = pics.map((p, i) => Math.max(p - 18, bandes[i]));
  }

  // ── Affichage ───────────────────────────────────────────────────────
  function fmt(s: number | null): string {
    if (s == null || !isFinite(s) || s < 0) return "--:--";
    const m = Math.floor(s / 60);
    return m + ":" + String(Math.floor(s % 60)).padStart(2, "0");
  }

  let dureeS = $derived(snap?.dur ?? null);
  let posS = $derived(posMs / 1000);
  let pct = $derived(
    dureeS != null && dureeS > 0 ? Math.min(100, (posS / dureeS) * 100) : 0
  );
  let lcdTexte = $derived(
    enCours
      ? `♪ ${enCours.titre} — @${enCours.demandeur} [${enCours.plateforme}]`
      : "—"
  );

  function onVolume(v: number): void {
    void musiqueVolume(v / 100);
  }
</script>

<div class="winamp" class:shade>
  <!-- Barre de titre : dégradé nuit + logo + boutons (_ = shade réel,
       les deux autres sont décoratifs comme sur le skin classic).
       Double-clic = bascule shade aussi (le bouton _ reste l'unique
       cible clavier, d'où le svelte-ignore). -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="wtitre" ondblclick={() => (shade = !shade)}>
    <span class="wt-logo">WINAMP</span>
    <span class="wt-btns">
      <button
        class="wt-btn"
        title={$t("interactions.musique.shade")}
        onclick={() => (shade = !shade)}>{shade ? "❐" : "_"}</button
      >
      <span class="wt-btn wt-ghost" aria-hidden="true">▪</span>
      <span class="wt-btn wt-ghost" aria-hidden="true">✕</span>
    </span>
  </div>

  {#if !shade}
    <!-- Zone principale : clutterbar + compteur LED + LCD/indicateurs -->
    <div class="main">
      <div class="clutter" aria-hidden="true">
        <span>O</span><span>A</span><span>I</span><span>D</span><span>V</span>
      </div>
      <div class="lcd-temps">{fmt(posS)}</div>
      <div class="lcd">
        <div class="lcd-marquee">
          <div class="lcd-defile">
            <span>{lcdTexte}</span>
            <span>{lcdTexte}</span>
          </div>
        </div>
        <div class="lcd-specs">
          <span class="spec">kbps <b>320</b></span>
          <span class="spec">kHz <b>44</b></span>
          <span class="spec lamp" class:on={!!enCours}>stereo</span>
          <span class="spec lamp">mono</span>
        </div>
      </div>
    </div>

    <!-- Équalizer spectre + peak dots -->
    <div class="eq" aria-hidden="true">
      {#each bandes as b, i (i)}
        <div class="eq-col">
          {#if pics[i] > b + 6}
            <div
              class="eq-peak"
              style:bottom="{Math.min(100, Math.round((pics[i] / 255) * 100))}%"
            ></div>
          {/if}
          <div class="eq-bar" style:height="{Math.max(4, Math.round((b / 255) * 100))}%"></div>
        </div>
      {/each}
    </div>

    <!-- Barre de progression (affichage seul — pas de seek en v1) -->
    <div class="prog">
      <div class="prog-fill" style:width="{pct}%"></div>
    </div>

    <!-- Transport : ⏮ rejouer · ⏯ pause · ⏹ stop · ⏭ skip · ⏏ import -->
    <div class="transport">
      <button
        class="wbtn"
        title={$t("interactions.musique.rejouer")}
        disabled={!enCours}
        onclick={() => enCours && void testerMusique(enCours.src)}>⏮</button
      >
      <button
        class="wbtn"
        title={pausee ? $t("interactions.musique.reprendre") : $t("interactions.musique.pause")}
        disabled={!enCours}
        onclick={() => void musiquePause(!pausee)}
      >{pausee ? "▶" : "⏸"}</button>
      <button
        class="wbtn danger"
        title={$t("interactions.musique.stop")}
        disabled={!enCours}
        onclick={() => (confirmStop = true)}
      >⏹</button>
      <button
        class="wbtn"
        title={$t("interactions.musique.skip")}
        disabled={!enCours && file.length === 0}
        onclick={() => void musiqueSkip()}
      >⏭</button>
      <button
        class="wbtn"
        title={$t("interactions.musique.importer")}
        onclick={() => void importerMusique()}
      >⏏</button>
      <button
        class="wbtn"
        title={$t("interactions.musique.vider")}
        disabled={file.length === 0}
        onclick={() => void musiqueVider()}
      >✕≡</button>
      <span class="wvol">
        🔊
        <input
          type="range"
          min="0"
          max="100"
          value={Math.round(etat.config.volume * 100)}
          oninput={(e) => onVolume(Number(e.currentTarget.value))}
        />
      </span>
      <span class="wactif" class:on={etat.config.actif}>
        {etat.config.actif ? "ACTIF" : "OFF"}
      </span>
    </div>

    <!-- Confirmation stop inline (sous le transport, visible tout de
         suite — pas de bandeau lointain en bas de modale). -->
    {#if confirmStop}
      <div class="wconfirm">
        <span class="wconfirm-txt">{$t("interactions.musique.confirmStop")}</span>
        <button
          class="wbtn danger"
          onclick={() => {
            confirmStop = false;
            void musiqueStop();
          }}>{$t("interactions.musique.confirmer")}</button
        >
        <button class="wbtn" onclick={() => (confirmStop = false)}>
          {$t("interactions.musique.annuler")}
        </button>
      </div>
    {/if}

    <!-- Playlist : piste courante surlignée en #1, puis la file. -->
    <div class="pl">
      {#if enCours}
        <div class="pl-row courant">
          <span class="pl-pos">1▸</span>
          <span class="pl-titre">{enCours.titre}</span>
          <span class="pl-meta">@{enCours.demandeur} [{enCours.plateforme}]</span>
        </div>
      {/if}
      {#each file as piste, i (piste.id)}
        <div class="pl-row">
          <span class="pl-pos">{i + (enCours ? 2 : 1)}</span>
          <span class="pl-titre">{piste.titre}</span>
          <span class="pl-meta">@{piste.demandeur}</span>
          <span class="pl-actions">
            <button
              class="pl-btn"
              disabled={i === 0}
              title={$t("interactions.musique.monter")}
              onclick={() => void musiqueDeplacer(piste.id, -1)}>↑</button
            >
            <button
              class="pl-btn"
              disabled={i === file.length - 1}
              title={$t("interactions.musique.descendre")}
              onclick={() => void musiqueDeplacer(piste.id, 1)}>↓</button
            >
            <button
              class="pl-btn danger"
              title={$t("interactions.musique.retirer")}
              onclick={() => void musiqueRetirer(piste.id)}>✕</button
            >
          </span>
        </div>
      {/each}
      {#if !enCours && file.length === 0}
        <div class="pl-vide">{$t("interactions.musique.fileVide")}</div>
      {/if}
    </div>

    <!-- Chips cooldowns actifs -->
    {#if cds.length > 0}
      <div class="cds">
        {#each cds as c (c.pseudo)}
          <span class="cd">CD {c.pseudo} {c.restant}s</span>
        {/each}
      </div>
    {/if}
  {/if}
</div>

<style>
  /* ===== CONTRAT VISUEL « skin classic » Winamp =====
     Ce deck est la palette de référence partagée avec diffusion.html
     (widget musique .mu-*) et WidgetMusique.svelte (vignette canvas) :
     châssis #2e2e2e→#1c1c1c + bevel outset, écrans LCD #0a140a / LED
     #050805 + border inset #444, LED verte #4aff4a, EQ #ff4040→#ffb040→
     #4aff4a + peak #ff9c40, progression crantée + fill #7dff7d→#2fd32f,
     police "Lucida Console". Toute retouche se reporte aux deux autres. */

  /* ── Châssis Winamp classic : dégradé métal sombre + bevel ── */
  .winamp {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 8px;
    border: 2px outset #555;
    border-radius: 3px;
    background: linear-gradient(180deg, #2e2e2e 0%, #1c1c1c 100%);
    box-shadow:
      inset 0 1px 0 rgba(255, 255, 255, 0.08),
      0 2px 6px rgba(0, 0, 0, 0.5);
    font-family: "Lucida Console", Consolas, monospace;
    user-select: none;
  }
  .winamp.shade {
    padding: 4px;
    gap: 0;
  }

  /* ── Barre de titre : dégradé bleu nuit + logo italique + boutons ── */
  .wtitre {
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: 15px;
    padding: 0 3px;
    border: 1px solid #111;
    border-bottom-color: #000;
    border-radius: 2px;
    background: linear-gradient(
      90deg,
      #000028 0%,
      #1d1d6e 45%,
      #4a4ab8 62%,
      #000028 100%
    );
  }
  .wt-logo {
    font-size: 0.55rem;
    font-weight: 700;
    font-style: italic;
    letter-spacing: 0.2em;
    color: #d5d6ff;
    text-shadow: 0 0 4px rgba(150, 150, 255, 0.6);
  }
  .wt-btns {
    display: flex;
    gap: 2px;
  }
  .wt-btn {
    width: 11px;
    height: 9px;
    padding: 0;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    font-size: 6px;
    line-height: 1;
    font-family: inherit;
    border: 1px outset #888;
    border-radius: 1px;
    background: linear-gradient(180deg, #3d3d3d, #222);
    color: #bbb;
    cursor: pointer;
  }
  button.wt-btn:hover {
    color: #fff;
    border-color: #aaa;
  }
  .wt-btn.wt-ghost {
    cursor: default;
    color: #777;
  }

  /* ── Zone principale : clutter + compteur LED + LCD ── */
  .main {
    display: flex;
    align-items: stretch;
    gap: 6px;
  }
  .clutter {
    display: flex;
    flex-direction: column;
    justify-content: space-around;
    padding: 3px 0;
    font-size: 6px;
    font-weight: 700;
    line-height: 1;
    color: #464646;
  }
  .lcd-temps {
    min-width: 4.4em;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 0 4px;
    border: 2px inset #444;
    border-radius: 2px;
    background: #050805;
    color: #4aff4a;
    text-shadow: 0 0 7px rgba(74, 255, 74, 0.65);
    font-size: 1.3rem;
    font-weight: 700;
    font-variant-numeric: tabular-nums;
    letter-spacing: 0.04em;
  }
  .lcd {
    flex: 1;
    min-width: 0;
    border: 2px inset #444;
    border-radius: 2px;
    background: #0a140a;
    padding: 4px 6px;
    display: flex;
    flex-direction: column;
    gap: 2px;
    overflow: hidden;
  }
  .lcd-marquee {
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
  }
  .lcd-defile {
    display: inline-block;
    white-space: nowrap;
    animation: lcd-scroll 12s linear infinite;
  }
  .lcd-defile span {
    color: #4aff4a;
    text-shadow: 0 0 6px rgba(74, 255, 74, 0.6);
    font-weight: 700;
    font-size: 0.78rem;
    padding-right: 3em;
  }
  @keyframes lcd-scroll {
    from {
      transform: translateX(0);
    }
    to {
      transform: translateX(-50%);
    }
  }
  .lcd-specs {
    display: flex;
    align-items: center;
    gap: 10px;
    font-size: 0.58rem;
    font-weight: 700;
    color: #2f6e2f;
  }
  .lcd-specs b {
    color: #4aff4a;
    font-weight: 700;
  }
  .lamp {
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: #24401f;
  }
  .lamp.on {
    color: #4aff4a;
    text-shadow: 0 0 5px rgba(74, 255, 74, 0.7);
  }

  /* ── Équalizer : 16 colonnes, barres montant du bas + peak dots ── */
  .eq {
    display: flex;
    align-items: flex-end;
    gap: 2px;
    height: 42px;
    padding: 3px 4px;
    border: 2px inset #444;
    border-radius: 2px;
    background: #0a140a;
  }
  .eq-col {
    position: relative;
    flex: 1;
    height: 100%;
    display: flex;
    align-items: flex-end;
  }
  .eq-peak {
    position: absolute;
    left: 0;
    right: 0;
    height: 2px;
    background: #ff9c40;
    box-shadow: 0 0 3px rgba(255, 156, 64, 0.6);
  }
  .eq-bar {
    width: 100%;
    background: linear-gradient(180deg, #ff4040 0%, #ffb040 30%, #4aff4a 65%);
    box-shadow: 0 0 3px rgba(74, 255, 74, 0.35);
    transition: height 90ms linear;
  }

  /* ── Barre de progression crantée ── */
  .prog {
    height: 8px;
    border: 1px inset #444;
    border-radius: 2px;
    background:
      repeating-linear-gradient(
        90deg,
        transparent 0,
        transparent 9px,
        rgba(0, 0, 0, 0.55) 9px,
        rgba(0, 0, 0, 0.55) 10px
      ),
      #101410;
    overflow: hidden;
  }
  .prog-fill {
    height: 100%;
    background: linear-gradient(180deg, #7dff7d 0%, #2fd32f 100%);
    box-shadow: 0 0 5px rgba(74, 255, 74, 0.5);
    transition: width 250ms linear;
  }

  /* ── Transport : boutons bevel + slider stylé ── */
  .transport {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .wbtn {
    border: 2px outset #666;
    border-radius: 2px;
    background: linear-gradient(180deg, #3a3a3a, #262626);
    color: #d8d8d8;
    font-family: inherit;
    font-size: 0.85rem;
    padding: 2px 9px;
    cursor: pointer;
    line-height: 1.4;
  }
  .wbtn:hover:not(:disabled) {
    border-color: #888;
    color: #fff;
  }
  .wbtn:active:not(:disabled) {
    border-style: inset;
  }
  .wbtn:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
  .wbtn.danger {
    color: #ff7a7a;
  }
  .wvol {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    color: #b8b8b8;
    font-size: 0.75rem;
  }
  .wvol input[type="range"] {
    width: 90px;
    appearance: none;
    -webkit-appearance: none;
    height: 6px;
    border: 1px inset #444;
    background: #101410;
    border-radius: 2px;
  }
  .wvol input[type="range"]::-webkit-slider-thumb {
    -webkit-appearance: none;
    width: 10px;
    height: 16px;
    border: 1px outset #777;
    border-radius: 2px;
    background: linear-gradient(180deg, #4a4a4a, #2c2c2c);
    cursor: pointer;
  }
  .wvol input[type="range"]::-moz-range-thumb {
    width: 10px;
    height: 16px;
    border: 1px outset #777;
    border-radius: 2px;
    background: linear-gradient(180deg, #4a4a4a, #2c2c2c);
    cursor: pointer;
  }
  .wactif {
    margin-left: auto;
    font-size: 0.65rem;
    font-weight: 700;
    padding: 1px 6px;
    border: 1px inset #444;
    border-radius: 2px;
    color: #777;
    background: #141414;
  }
  .wactif.on {
    color: #4aff4a;
    text-shadow: 0 0 5px rgba(74, 255, 74, 0.6);
  }

  /* ── Bandeau de confirmation stop (inline, sous le transport) ── */
  .wconfirm {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 3px 6px;
    border: 1px inset #444;
    border-radius: 2px;
    background: #141414;
    font-size: 0.7rem;
  }
  .wconfirm-txt {
    flex: 1;
    color: #ff9c40;
    font-weight: 700;
  }

  /* ── Playlist : file sur fond LCD noir, piste courante surlignée ── */
  .pl {
    border: 2px inset #444;
    border-radius: 2px;
    background: #050805;
    max-height: 104px;
    overflow-y: auto;
    font-size: 0.7rem;
    font-weight: 700;
  }
  .pl-row {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 1px 6px;
    color: #3fbf3f;
  }
  .pl-row.courant {
    color: #8dff8d;
    text-shadow: 0 0 5px rgba(74, 255, 74, 0.55);
  }
  .pl-pos {
    min-width: 1.8em;
    font-variant-numeric: tabular-nums;
    color: #2a7a2a;
  }
  .pl-row.courant .pl-pos {
    color: #4aff4a;
  }
  .pl-titre {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .pl-meta {
    font-size: 0.6rem;
    color: #2a7a2a;
    white-space: nowrap;
  }
  .pl-actions {
    display: flex;
    gap: 2px;
  }
  .pl-btn {
    padding: 0 4px;
    font-size: 0.65rem;
    font-family: inherit;
    line-height: 1.3;
    border: 1px outset #555;
    border-radius: 2px;
    background: linear-gradient(180deg, #333, #202020);
    color: #9adf9a;
    cursor: pointer;
  }
  .pl-btn:hover:not(:disabled) {
    border-color: #777;
    color: #dfffdf;
  }
  .pl-btn:disabled {
    opacity: 0.35;
    cursor: not-allowed;
  }
  .pl-btn.danger {
    color: #ff8a8a;
  }
  .pl-vide {
    padding: 5px 8px;
    font-size: 0.62rem;
    color: #2a7a2a;
  }

  /* ── Chips cooldowns ── */
  .cds {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .cd {
    font-size: 0.65rem;
    font-weight: 700;
    padding: 1px 6px;
    border: 1px inset #444;
    border-radius: 2px;
    color: #ffb040;
    background: #141414;
    font-variant-numeric: tabular-nums;
  }
</style>
