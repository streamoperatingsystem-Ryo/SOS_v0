<script lang="ts">
  // Widget Speedrun Splitter (dashboard, lecture seule). Affiche le timer,
  // la phase, la barre de progression et les segments LSS. Les contrôles
  // (import ASL/LSS, Start/Stop, Split/Skip/Undo/Reset/Pause, toggles Auto
  // Start/Split/Reset) sont dans la Toolbar (panneau latéral gauche), comme
  // pour les widgets chat / caméra / input-viewer. Le rendu diffusion est
  // géré par diffusion.html via WS (speedrun-etat), pas ici.
  import {
    phaseTimer,
    tempsJeu,
    tempsReel,
    indexSplitCourant,
    totalSplits,
    segmentsLss,
    splits,
    nomJeuLss,
    nomCategorieLss,
    estActif,
    estConnecte,
    nomProcessus,
    erreurSpeedrun,
    chargerAsl,
    chargerLss,
  } from "../stores/speedrun";

  // Props
  // gradient : paire {from, to} du cadre de scène actif (fallback blanc→gris).
  // Le timer et les bordures des segments reprennent ce gradient (comme le titre).
  let {
    contenu,
    gradient = { from: "#e0e0e0", to: "#9a9a9a" },
  }: { contenu: any; gradient?: { from: string; to: string } } = $props();

  // Police Google Font + taille de base du splitter (comme le chat). Toutes les
  // tailles internes sont en em → suit srTaillePolice. Défaut Rajdhani 27px.
  let srPolice = $derived(contenu?.srPolice ?? "Rajdhani");
  let srTaille = $derived(contenu?.srTaillePolice ?? 27);

  // Ombre de contraste (noir ou blanc) selon la luminance de la couleur du
  // cadre : fait ressortir le timer (gradient) quel que soit le fond. Texte
  // clair → ombre noire ; texte sombre → ombre blanche.
  function ombreContraste(hex: string): string {
    if (!hex || hex[0] !== "#") return "#000";
    const c = hex.length === 4
      ? "#" + hex[1] + hex[1] + hex[2] + hex[2] + hex[3] + hex[3]
      : hex;
    const r = parseInt(c.slice(1, 3), 16) / 255;
    const g = parseInt(c.slice(3, 5), 16) / 255;
    const b = parseInt(c.slice(5, 7), 16) / 255;
    const lum = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    return lum > 0.5 ? "#000" : "#fff";
  }
  let srOmbre = $derived(ombreContraste(gradient.from));

  // Rechargement auto au montage si chemins présents dans le widget : restaure
  // l'état du moteur (segments + noms) pour le timer dashboard. Les contrôles
  // sont dans la Toolbar, mais le rechargement reste ici (le widget connaît
  // ses chemins persistés).
  $effect(() => {
    if (contenu?.speedrunCheminAsl) {
      chargerAsl(contenu.speedrunCheminAsl).catch(() => {});
    }
    if (contenu?.speedrunCheminLss) {
      chargerLss(contenu.speedrunCheminLss).catch(() => {});
    }
  });

  // Temps affiché : game time si disponible, sinon real time (RTA)
  let tempsAffiche = $derived($tempsJeu ?? $tempsReel);

  // Helpers temps : parse "MM:SS.mmm" → secondes, et format secondes → "MM:SS.cc".
  function parseTime(str: string | null | undefined): number {
    if (!str || str === "--/--/--") return 0;
    const parts = str.split(":");
    let h = 0, m = 0, s = 0;
    if (parts.length === 3) {
      h = parseFloat(parts[0]) || 0;
      m = parseFloat(parts[1]) || 0;
      s = parseFloat(parts[2]) || 0;
    } else if (parts.length === 2) {
      m = parseFloat(parts[0]) || 0;
      s = parseFloat(parts[1]) || 0;
    } else {
      s = parseFloat(parts[0]) || 0;
    }
    return h * 3600 + m * 60 + s;
  }
  function formatSegment(secs: number): string {
    if (secs < 0 || isNaN(secs)) return "--/--/--";
    const totalCs = Math.round(secs * 100);
    const h = Math.floor(totalCs / 360000);
    const m = Math.floor((totalCs % 360000) / 6000);
    const s = Math.floor((totalCs % 6000) / 100);
    const cs = totalCs % 100;
    if (h > 0) {
      return `${h}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}.${String(cs).padStart(2, "0")}`;
    }
    return `${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}.${String(cs).padStart(2, "0")}`;
  }

  // Temps de segment (delta, pas cumulé) pour chaque segment i.
  //   - passé : cumul[i] - cumul[i-1]
  //   - actif : tempsCourant - cumul[i-1]
  //   - non parcouru : "--/--/--"
  function tempsSegment(i: number): string {
    const idx = $indexSplitCourant;
    if (idx === null) return "--/--/--";
    if (i < idx) {
      const cumulI = parseTime($splits[i]);
      const cumulPrev = i > 0 ? parseTime($splits[i - 1]) : 0;
      return formatSegment(cumulI - cumulPrev);
    }
    if (i === idx) {
      const current = parseTime(tempsAffiche);
      const cumulPrev = i > 0 ? parseTime($splits[i - 1]) : 0;
      return formatSegment(current - cumulPrev);
    }
    return "--/--/--";
  }

  // PB de segment (delta cumulé PB) pour les segments parcourus/actifs.
  //   - non parcouru : "--/--/--"
  function pbSegment(i: number): string {
    const idx = $indexSplitCourant;
    if (idx === null || i > idx) return "--/--/--";
    const seg = $segmentsLss[i];
    if (!seg) return "--/--/--";
    const pbI = seg.pbRealTime != null ? Number(seg.pbRealTime) : NaN;
    if (isNaN(pbI)) return "--/--/--";
    const pbPrev = (i > 0 && $segmentsLss[i - 1]?.pbRealTime != null)
      ? Number($segmentsLss[i - 1]!.pbRealTime) : 0;
    return formatSegment(pbI - pbPrev);
  }

  // Phase label court
  let phaseLabel = $derived.by(() => {
    switch ($phaseTimer) {
      case "Running": return "Running";
      case "Paused": return "Paused";
      case "Ended": return "Finished";
      default: return "Ready";
    }
  });
  let phaseCls = $derived.by(() => {
    switch ($phaseTimer) {
      case "Running": return "running";
      case "Paused": return "paused";
      case "Ended": return "ended";
      default: return "ready";
    }
  });

  // Progression 0→1 pour la barre
  let progression = $derived.by(() => {
    if ($totalSplits <= 0) return 0;
    if ($indexSplitCourant === null) return 0;
    return Math.min(1, $indexSplitCourant / $totalSplits);
  });

  // Ref du conteneur de segments pour le scroll auto.
  let segmentsEl = $state<HTMLDivElement | null>(null);

  // Scroll auto : centrer le segment actif dans la liste quand l'index change.
  // Pas de smooth behavior (saccade dans le dashboard Tauri webview).
  // Au reset/arrêt (idx === null), remonter en haut pour voir le 1er split
  // au prochain démarrage.
  $effect(() => {
    const idx = $indexSplitCourant;
    if (!segmentsEl) return;
    if (idx === null) {
      segmentsEl.scrollTop = 0;
      return;
    }
    const segs = segmentsEl.querySelectorAll(".speedrun-segment");
    if (idx >= 0 && idx < segs.length) {
      const actif = segs[idx] as HTMLElement;
      const containerH = segmentsEl.clientHeight;
      const target = actif.offsetTop - (containerH - actif.offsetHeight) / 2;
      if (target >= 0 && Math.abs(segmentsEl.scrollTop - target) > 1) {
        segmentsEl.scrollTop = target;
      }
    }
  });
</script>

<div
  class="speedrun-widget"
  style="--sr-grad-from:{gradient.from}; --sr-grad-to:{gradient.to}; --sr-ombre:{srOmbre}; font-family: '{srPolice}', sans-serif; font-size: {srTaille}px;"
>
  {#if $erreurSpeedrun}
    <div class="speedrun-erreur">⚠ {$erreurSpeedrun}</div>
  {/if}

  <!-- En-tête : jeu + catégorie + statut connexion -->
  <div class="sr-header">
    <div class="sr-titre-jeu">
      {#if $nomJeuLss || $nomCategorieLss}
        {$nomJeuLss}{#if $nomCategorieLss}<span class="sr-cat"> — {$nomCategorieLss}</span>{/if}
      {:else}
        <span class="sr-placeholder">Aucun splits chargé</span>
      {/if}
    </div>
    {#if $estConnecte}
      <span class="sr-status ok" title="{$nomProcessus ?? "connecté"}">●</span>
    {:else if $estActif}
      <span class="sr-status attente" title="en attente…">○</span>
    {/if}
  </div>

  <!-- Timer : couleur pleine du cadre + ombre de contraste simple -->
  <div class="sr-timer" class:running={phaseCls === "running"} class:paused={phaseCls === "paused"} class:ended={phaseCls === "ended"} class:ready={phaseCls === "ready"}>
    <span class="sr-timer-valeur">{tempsAffiche}</span>
  </div>

  <!-- Phase pill + progression -->
  <div class="sr-meta">
    <span class="sr-phase-pill {phaseCls}">{phaseLabel}</span>
    {#if $totalSplits > 0}
      <span class="sr-compteur">
        {$indexSplitCourant !== null ? $indexSplitCourant : 0} / {$totalSplits}
      </span>
    {/if}
  </div>

  <!-- Barre de progression -->
  {#if $totalSplits > 0}
    <div class="sr-barre">
      <div class="sr-barre-fill" style="width:{progression * 100}%"></div>
    </div>
  {/if}

  <!-- Segments LSS -->
  {#if $segmentsLss.length > 0}
    <div class="speedrun-segments" bind:this={segmentsEl}>
      {#each $segmentsLss as seg, i}
        <div
          class="speedrun-segment"
          class:actif={$indexSplitCourant === i}
          class:done={$indexSplitCourant !== null && $indexSplitCourant > i}
        >
          <span class="seg-idx">{i + 1}</span>
          <span class="seg-nom">{seg.nom}</span>
          <span class="seg-pb">{pbSegment(i)}</span>
          <span class="seg-time">{tempsSegment(i)}</span>
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .speedrun-widget {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 10px;
    color: var(--texte);
    overflow: hidden;
    height: 100%;
    box-sizing: border-box;
  }

  /* ===== En-tête ===== */
  .sr-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 6px;
  }
  .sr-titre-jeu {
    font-size: 0.75em;
    font-weight: 600;
    color: var(--texte);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    /* Ombre de lisibilité : le titre est clair sur fond sombre → ombre noire. */
    text-shadow: 0 1px 2px #000, 0 0 1px #000;
  }
  .sr-cat {
    color: var(--gris);
    font-weight: 400;
  }
  .sr-placeholder {
    color: var(--gris);
    font-style: italic;
    font-weight: 400;
  }
  .sr-status {
    font-size: 0.7em;
    flex-shrink: 0;
  }
  .sr-status.ok { color: var(--message-ok-color); }
  .sr-status.attente { color: var(--message-user-action-color); }

  /* ===== Timer ===== */
  /* Couleur pleine du cadre (--sr-grad-from) + ombre de contraste simple
     (--sr-ombre : noir ou blanc selon la luminance du cadre). Une seule
     couche — lisible sur tout fond. */
  .sr-timer {
    text-align: center;
    padding: 4px 0 2px;
  }
  .sr-timer-valeur {
    font-size: 2.16em;
    font-weight: 700;
    font-variant-numeric: tabular-nums;
    letter-spacing: 0.04em;
    color: var(--sr-grad-from);
    text-shadow: 0 2px 4px var(--sr-ombre, #000);
    line-height: 1;
  }
  /* Glow subtil selon la phase (par-dessus l'ombre de contraste) */
  .sr-timer.running .sr-timer-valeur {
    filter: drop-shadow(0 0 6px color-mix(in srgb, var(--sr-grad-from) 40%, transparent));
  }
  .sr-timer.ended .sr-timer-valeur {
    filter: drop-shadow(0 0 8px color-mix(in srgb, var(--sr-grad-from) 50%, transparent));
  }

  /* ===== Phase pill + compteur ===== */
  .sr-meta {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
  }
  .sr-phase-pill {
    font-size: 0.65em;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    padding: 1px 8px;
    border-radius: 999px;
    color: var(--gris);
    background: color-mix(in srgb, var(--gris) 15%, transparent);
  }
  .sr-phase-pill.running {
    color: var(--message-ok-color);
    background: color-mix(in srgb, var(--message-ok-color) 15%, transparent);
  }
  .sr-phase-pill.paused {
    color: var(--message-user-action-color);
    background: color-mix(in srgb, var(--message-user-action-color) 15%, transparent);
  }
  .sr-phase-pill.ended {
    color: var(--message-ok-color);
    background: color-mix(in srgb, var(--message-ok-color) 20%, transparent);
  }
  .sr-compteur {
    font-size: 0.7em;
    color: var(--gris);
    font-variant-numeric: tabular-nums;
    text-shadow: 0 1px 2px #000;
  }

  /* ===== Barre de progression ===== */
  .sr-barre {
    height: 3px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--gris) 20%, transparent);
    overflow: hidden;
  }
  .sr-barre-fill {
    height: 100%;
    border-radius: 999px;
    background: linear-gradient(90deg, var(--sr-grad-from), var(--sr-grad-to));
    transition: width 0.3s ease;
  }

  /* ===== Segments ===== */
  .speedrun-segments {
    display: flex;
    flex-direction: column;
    gap: 3px;
    flex: 1;
    overflow-y: auto;
    min-height: 0;
    padding-top: 4px;
    /* Scrollbar thin — discrète sur fond sombre. */
    scrollbar-width: thin;
    scrollbar-color: rgba(255, 255, 255, 0.2) transparent;
  }
  .speedrun-segments::-webkit-scrollbar { width: 5px; }
  .speedrun-segments::-webkit-scrollbar-thumb {
    background: rgba(255, 255, 255, 0.2);
    border-radius: 999px;
  }
  .speedrun-segments::-webkit-scrollbar-track { background: transparent; }

  .speedrun-segment {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 3px 6px;
    font-size: 0.78em;
    border: 1px solid transparent;
    border-radius: var(--rayon-petit);
    /* BG gradient : on garde la couleur du user (variable) mais on remplace la
       couleur claire par du noir profond → dégradé couleur user → noir. La
       teinte est assombrie (35% sur noir) pour les splits non actifs afin que
       l'actif ressorte. */
    background: linear-gradient(135deg, color-mix(in srgb, var(--sr-grad-from) 35%, #000), #000000);
    transition: background 0.15s ease, box-shadow 0.15s ease, border-color 0.15s ease;
  }
  .speedrun-segment.actif {
    /* Split en cours : dégradé couleur user pleine → noir + cadre fluo
       lumineux de la couleur variable (glow externe + interne). */
    background: linear-gradient(135deg, var(--sr-grad-from), #000000);
    border-color: var(--sr-grad-from);
    box-shadow:
      0 0 10px 2px var(--sr-grad-from),
      inset 0 0 8px color-mix(in srgb, var(--sr-grad-from) 25%, transparent);
    color: var(--texte);
  }
  .speedrun-segment.done {
    color: var(--gris);
    opacity: 0.6;
  }
  .seg-idx {
    font-size: 0.85em;
    font-weight: 700;
    color: var(--gris);
    min-width: 1.4em;
    text-align: right;
    font-variant-numeric: tabular-nums;
    text-shadow: 0 1px 2px #000;
  }
  .speedrun-segment.actif .seg-idx {
    color: var(--sr-grad-from);
    text-shadow: 0 0 6px var(--sr-grad-from), 0 1px 2px #000;
  }
  .seg-nom {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    text-shadow: 0 1px 2px #000, 0 0 1px #000;
  }
  .seg-pb {
    font-variant-numeric: tabular-nums;
    color: var(--gris);
    font-size: 0.9em;
    text-shadow: 0 1px 2px #000;
  }
  /* Temps de run (split réalisé) — couleur du cadre pour le distinguer du
     PB (gris). Affiché pour les segments déjà passés. */
  .seg-time {
    font-variant-numeric: tabular-nums;
    color: var(--sr-grad-from);
    font-size: 0.9em;
    font-weight: 600;
    text-shadow: 0 1px 2px #000;
  }
  /* Split actif : temps en couleur cadre + glow doux (une seule couche
     lumineuse + ombre noire de lisibilité). */
  .speedrun-segment.actif .seg-time {
    color: var(--sr-grad-from);
    font-weight: 700;
    text-shadow:
      0 0 0.4em var(--sr-grad-from),
      0 1px 2px #000;
  }

  /* ===== Erreur ===== */
  .speedrun-erreur {
    color: var(--message-error-color);
    font-size: 0.8em;
    padding: 4px 8px;
    border-radius: var(--rayon-petit);
    background: color-mix(in srgb, var(--message-error-color) 10%, transparent);
  }
</style>
