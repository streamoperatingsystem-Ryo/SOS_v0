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
    referenceSplits,
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

  // Formate un delta signé façon LiveSplit (colonne "+/−") : signe toujours
  // présent, précision au dixième — "-1.6", "+2.3", "-1:23.4", "+1:02:03.4".
  function formatDelta(secs: number): string {
    const sign = secs < 0 ? "-" : "+";
    const totalDs = Math.round(Math.abs(secs) * 10);
    const h = Math.floor(totalDs / 36000);
    const m = Math.floor((totalDs % 36000) / 600);
    const s = Math.floor((totalDs % 600) / 10);
    const d = totalDs % 10;
    if (h > 0) {
      return `${sign}${h}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}.${d}`;
    }
    if (m > 0) {
      return `${sign}${m}:${String(s).padStart(2, "0")}.${d}`;
    }
    return `${sign}${s}.${d}`;
  }

  // Delta vs référence « dernière fois » (baseline mergée PB) pour la 1ère
  // colonne de chaque segment — CUMULÉ au split, comme LiveSplit :
  //   - passé : cumulRun[i] - ref[i]
  //   - actif : tempsCourant - ref[i]  (live à chaque tick)
  //   - non parcouru / skip / pas de référence : "0.0" neutre (compteurs à
  //     zéro — premier run sans sauvegarde)
  // sens : "ahead" (vert, devant) | "behind" (rouge, derrière) | "neutre".
  function deltaInfo(i: number): { texte: string; sens: "ahead" | "behind" | "neutre" } {
    const idx = $indexSplitCourant;
    const ref = $referenceSplits[i];
    if (idx === null || i > idx || ref == null) return { texte: "0.0", sens: "neutre" };
    let cumul: number | null = null;
    if (i < idx) {
      // Split passé : temps enregistré (vide si le segment a été skippé).
      const t = $splits[i];
      if (t) cumul = parseTime(t);
    } else {
      cumul = parseTime(tempsAffiche);
    }
    if (cumul === null) return { texte: "0.0", sens: "neutre" };
    // Classer sur la valeur ARRONDIE au dixième (cohérent avec l'affichage :
    // un delta affiché "0.0"/"-0.0" ne bascule pas de couleur).
    const d = Math.round((cumul - ref) * 10) / 10;
    return { texte: formatDelta(d), sens: d < 0 ? "ahead" : d > 0 ? "behind" : "neutre" };
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

  <!-- En-tête : titre du jeu centré en haut, catégorie juste en dessous,
       pastille de statut garée en coin (absolute). -->
  <div class="sr-header">
    {#if $nomJeuLss || $nomCategorieLss}
      <div class="sr-titre-jeu">{$nomJeuLss}</div>
      {#if $nomCategorieLss}
        <div class="sr-cat">{$nomCategorieLss}</div>
      {/if}
    {:else}
      <div class="sr-titre-jeu"><span class="sr-placeholder">Aucun splits chargé</span></div>
    {/if}
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
        {@const delta = deltaInfo(i)}
        <div
          class="speedrun-segment"
          class:actif={$indexSplitCourant === i}
          class:done={$indexSplitCourant !== null && $indexSplitCourant > i}
          class:seg-ahead={delta.sens === "ahead"}
          class:seg-behind={delta.sens === "behind"}
        >
          <span class="seg-idx">{i + 1}</span>
          <span class="seg-nom">{seg.nom}</span>
          <span class="seg-pb delta-{delta.sens}">{delta.texte}</span>
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
    /* Marge verticale de sécurité : le cadre SVG (overlay z-index 5) peut
       manger le haut/bas du contenu selon son épaisseur/inclinaison. En em
       → suit srTaillePolice. */
    padding: 0.8em 10px;
    color: var(--texte);
    overflow: hidden;
    height: 100%;
    box-sizing: border-box;
  }

  /* ===== En-tête ===== */
  .sr-header {
    display: flex;
    /* Titre centré tout en haut, catégorie empilée juste en dessous. */
    flex-direction: column;
    align-items: center;
    gap: 2px;
    /* Ancre pour la pastille de statut (garée en coin, hors flux). */
    position: relative;
  }
  .sr-titre-jeu {
    font-size: 0.75em;
    font-weight: 600;
    color: var(--texte);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    text-align: center;
    /* En colonne centrée l'item shrink-wrappe son contenu : max-width pour
       que les titres longs ellipsent au lieu de déborder. */
    max-width: 100%;
    /* Ombre de lisibilité : le titre est clair sur fond sombre → ombre noire. */
    text-shadow: 0 1px 2px #000, 0 0 1px #000;
  }
  /* Catégorie sur sa propre ligne (était un span inline « — cat » dans le
     titre). Un peu plus petite que le titre. */
  .sr-cat {
    color: var(--gris);
    font-weight: 400;
    font-size: 0.7em;
  }
  .sr-placeholder {
    color: var(--gris);
    font-style: italic;
    font-weight: 400;
  }
  .sr-status {
    /* Garée en coin haut-droit du header — hors du flux centré. */
    position: absolute;
    top: 0;
    right: 0;
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
    /* Marges L/R : inset face aux cadres épais. Fond « glace » teinté de la
       couleur du cadre (comme le glass des messages chat). */
    margin: 0 0.8em;
    padding: 0.15em 0.5em;
    background: color-mix(in srgb, var(--sr-grad-from) 15%, transparent);
    backdrop-filter: blur(8px);
    border-radius: var(--rayon-petit);
  }
  .sr-timer-valeur {
    font-size: 1.9em;
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
    padding: 6px;
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
  /* Colonne delta « dernière fois » (1ère colonne temps, ex-PB) : vert =
     temps gagné (devant la référence), rouge = temps perdu, gris = neutre
     ("0.0", pas de comparaison). Variables sémantiques — jamais d'hex. */
  .seg-pb {
    font-variant-numeric: tabular-nums;
    color: var(--gris);
    font-size: 0.9em;
    text-shadow: 0 1px 2px #000;
  }
  .seg-pb.delta-ahead { color: var(--message-ok-color); }
  .seg-pb.delta-behind { color: var(--message-error-color); }
  .seg-pb.delta-ahead,
  .seg-pb.delta-behind { font-weight: 700; }
  /* Bordure du split entier colorée selon le résultat (demande utilisateur) :
     verte si le split est gagné, rouge s'il est perdu. Déclaré APRÈS .actif
     (même spécificité) → le résultat l'emporte aussi sur le split en cours. */
  .speedrun-segment.seg-ahead { border-color: var(--message-ok-color); }
  .speedrun-segment.seg-behind { border-color: var(--message-error-color); }
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
