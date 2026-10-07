<script lang="ts">
  // Vignette dashboard du widget musique — MIROIR 1:1 du mini-lecteur
  // Winamp de diffusion.html (.widget.musique) : même markup mu-*, même
  // palette « skin classic », même dimensionnement par --mu-w/--mu-h
  // (le canvas rend le widget en px réels puis le scale — rendu
  // pixel-identique à OBS).
  //
  // Le son reste sur la diffusion (browser source OBS) : cette vignette
  // est une visualisation. La progression est un SNAPSHOT figé à l'emit
  // ({position_ms, pausee, duree_s}) avancé ici en tick local 250 ms ;
  // les bandes EQ viennent de "musique:spectrum" (AnalyserNode de l'audio
  // ombre côté diffusion, ~8 Hz) avec animation simulée en repli si
  // aucune frame n'arrive pendant la lecture — même logique que
  // LecteurMusique.svelte.
  import { onMount } from "svelte";
  import { get } from "svelte/store";
  import {
    initMusique,
    musiqueEtat,
    musiqueBands,
    musiqueBandsTs,
  } from "../stores/musique";
  import type { Widget } from "../tauri";

  let { w }: { w: Widget } = $props();

  const NB_BANDES = 16;

  let enCours = $derived($musiqueEtat?.en_cours ?? null);
  let pausee = $derived($musiqueEtat?.progression?.pausee ?? false);
  let eqOnly = $derived((w.musiqueMode ?? "player") === "eq");

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
    const p = $musiqueEtat?.progression;
    snap = p
      ? { pos: p.position_ms, pausee: p.pausee, dur: p.duree_s, at: Date.now() }
      : null;
  });

  // ── Valeurs affichées (rafraîchies par le tick 250 ms) ─────────────
  let posMs = $state(0);
  let bandes = $state<number[]>(new Array(NB_BANDES).fill(0));
  /// Peak dots : crête par bande retombant doucement (signature Winamp).
  let pics = $state<number[]>(new Array(NB_BANDES).fill(0));

  onMount(() => {
    void initMusique();
    const tick = setInterval(tickUi, 250);
    return () => clearInterval(tick);
  });

  function tickUi(): void {
    if (snap) {
      posMs = snap.pausee ? snap.pos : snap.pos + (Date.now() - snap.at);
      if (snap.dur != null) posMs = Math.min(posMs, snap.dur * 1000);
    } else {
      posMs = 0;
    }
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
  /// mm:ss depuis des secondes (--:-- si non fini/négatif) — même format
  /// que muFmt de diffusion.html.
  function muFmt(s: number | null): string {
    if (s == null || !isFinite(s) || s < 0) return "--:--";
    const m = Math.floor(s / 60);
    return m + ":" + String(Math.floor(s % 60)).padStart(2, "0");
  }

  let dureeS = $derived(snap?.dur ?? null);
  let posS = $derived(posMs / 1000);
  let restS = $derived(dureeS != null ? Math.max(0, dureeS - posS) : null);
  let pct = $derived(
    dureeS != null && dureeS > 0 ? Math.min(100, (posS / dureeS) * 100) : 0
  );
  let lcdTexte = $derived(
    enCours ? `♪ ${enCours.titre} — ${enCours.demandeur}` : "♪ —"
  );
  /// Durée marquee proportionnelle au titre (même formule que
  /// majMusiqueWidget : max(8, longueur × 0,22 s)).
  let defileDuree = $derived(Math.max(8, lcdTexte.length * 0.22));
</script>

<!-- Markup miroir de diffusion.html (widget musique, branche CREATE) :
     mu-titre / mu-lcd (LED temps + LCD marquee) / mu-eq / mu-posbar. -->
<div
  class="mu-root"
  class:mu-eq-only={eqOnly}
  style="--mu-w:{w.largeur}px; --mu-h:{w.hauteur}px;"
>
  <div class="mu-titre">WINAMP</div>
  <div class="mu-lcd">
    <span class="mu-temps"
      ><span class="mu-pos">{muFmt(posS)}</span><span class="mu-rest"
        >{restS != null ? " -" + muFmt(restS) : ""}</span
      ></span
    >
    <div class="mu-defile-wrap">
      <span class="mu-defile" style:animation-duration="{defileDuree}s">
        <span class="mu-txt">{lcdTexte}</span>
        <span class="mu-txt">{lcdTexte}</span>
      </span>
    </div>
  </div>
  <div class="mu-eq" aria-hidden="true">
    {#each bandes as b, i (i)}
      <div class="mu-col">
        {#if pics[i] > b + 6}
          <div
            class="mu-peak"
            style:bottom="{Math.min(100, Math.round((pics[i] / 255) * 100))}%"
          ></div>
        {/if}
        <div class="mu-bar" style:height="{Math.max(4, Math.round((b / 255) * 100))}%"></div>
      </div>
    {/each}
  </div>
  <div class="mu-posbar"><div class="mu-posfill" style:width="{pct}%"></div></div>
</div>

<style>
  /* ===== Vignette musique — MIROIR du bloc mu-* de diffusion.html =====
     CONTRAT VISUEL « skin classic » : toute évolution de la palette ou de
     la structure mu-* doit être reportée dans diffusion.html (référence)
     ET LecteurMusique.svelte (deck de l'onglet Musique). --mu-w/--mu-h
     posées inline depuis w.largeur/w.hauteur (même technique que
     diffusion — le canvas scale ensuite le widget entier). */

  /* ── Châssis métal sombre + bevel ── */
  .mu-root {
    position: absolute;
    inset: 0;
    z-index: 1; /* au-dessus du .media-fond éventuel (z-index 0) */
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: calc(var(--mu-h, 120px) * 0.06);
    padding: calc(var(--mu-h, 120px) * 0.07) calc(var(--mu-w, 340px) * 0.04);
    box-sizing: border-box;
    border: 2px outset #555;
    border-radius: 3px;
    background: linear-gradient(180deg, #2e2e2e 0%, #1c1c1c 100%);
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.08);
    color: #4aff4a;
    font-family: "Lucida Console", Consolas, ui-monospace, monospace;
    pointer-events: none;
  }

  /* ── Barre de titre : dégradé bleu nuit + logo italique ── */
  .mu-titre {
    display: flex;
    align-items: center;
    height: calc(var(--mu-h, 120px) * 0.11);
    min-height: 8px;
    padding: 0 6px;
    border: 1px solid #111;
    border-radius: 2px;
    background: linear-gradient(
      90deg,
      #000028 0%,
      #1d1d6e 45%,
      #4a4ab8 62%,
      #000028 100%
    );
    font-style: italic;
    font-weight: 700;
    letter-spacing: 0.2em;
    font-size: calc(var(--mu-h, 120px) * 0.08);
    color: #d5d6ff;
    text-shadow: 0 0 4px rgba(150, 150, 255, 0.6);
    overflow: hidden;
    white-space: nowrap;
  }

  /* ── Rangée écrans : boîte LED temps (gauche) + LCD marquee (droite) ── */
  .mu-lcd {
    display: flex;
    align-items: stretch;
    gap: calc(var(--mu-w, 340px) * 0.02);
    min-width: 0;
    font-size: calc(var(--mu-h, 120px) * 0.14);
    font-weight: 700;
    letter-spacing: 0.04em;
    text-shadow: 0 0 6px rgba(74, 255, 74, 0.55);
  }
  .mu-temps {
    display: flex;
    align-items: center;
    padding: calc(var(--mu-h, 120px) * 0.05) 0.45em;
    border: 2px inset #444;
    border-radius: 2px;
    background: #050805;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .mu-rest {
    color: #2ea02e;
    font-size: 0.62em;
  }
  /* Marquee : deux copies du texte + translateX -50% (boucle sans saut). */
  .mu-defile-wrap {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    padding: calc(var(--mu-h, 120px) * 0.05) 0.5em;
    border: 2px inset #444;
    border-radius: 2px;
    background: #0a140a;
    overflow: hidden;
    white-space: nowrap;
  }
  .mu-defile {
    display: inline-block;
    animation: mu-defil 12s linear infinite;
  }
  .mu-defile > span {
    padding-right: 3em;
  }
  @keyframes mu-defil {
    from {
      transform: translateX(0);
    }
    to {
      transform: translateX(-50%);
    }
  }

  /* ── Équalizer : 16 colonnes, barres montant du bas + peak dots ── */
  .mu-eq {
    display: flex;
    align-items: flex-end;
    gap: 3px;
    height: calc(var(--mu-h, 120px) * 0.3);
    min-height: 14px;
    border: 2px inset #444;
    border-radius: 2px;
    background: #0a140a;
    padding: 3px 4px;
    box-sizing: border-box;
  }
  .mu-col {
    position: relative;
    flex: 1;
    height: 100%;
    display: flex;
    align-items: flex-end;
  }
  .mu-peak {
    position: absolute;
    left: 0;
    right: 0;
    height: 2px;
    background: #ff9c40;
    box-shadow: 0 0 4px rgba(255, 156, 64, 0.6);
  }
  .mu-bar {
    width: 100%;
    height: 4%;
    min-height: 2px;
    background: linear-gradient(180deg, #ff4040 0%, #ffb040 30%, #4aff4a 65%);
    box-shadow: 0 0 3px rgba(74, 255, 74, 0.35);
    transition: height 80ms linear;
  }

  /* ── Barre de progression crantée ── */
  .mu-posbar {
    height: calc(var(--mu-h, 120px) * 0.07);
    min-height: 5px;
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
  .mu-posfill {
    height: 100%;
    width: 0%;
    background: linear-gradient(180deg, #7dff7d 0%, #2fd32f 100%);
    box-shadow: 0 0 5px rgba(74, 255, 74, 0.5);
    transition: width 250ms linear;
  }

  /* ── Mode "eq" : bandes seules occupant tout le widget. ── */
  .mu-root.mu-eq-only .mu-titre,
  .mu-root.mu-eq-only .mu-lcd,
  .mu-root.mu-eq-only .mu-posbar {
    display: none;
  }
  .mu-root.mu-eq-only .mu-eq {
    flex: 1;
    height: auto;
  }
</style>
