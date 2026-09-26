<script lang="ts">
  // ConsoModal — « Diagnostic » : consommation réelle mesurée par le backend
  // (conso.rs, 1 échantillon/s) : processus (StreamOS, WebView2, OBS, jeu,
  // système) + sous-systèmes (threads nommés : splitter, input viewer…) +
  // compteurs d'activité (chat/s, events speedrun/s, pubs input/s).
  //
  // Objectif : ultra lisible pour un non-technicien — une ligne = un nom,
  // sa valeur actuelle, son max récent, et sa courbe des 5 dernières minutes.
  // Le survol d'une courbe affiche le point précis (valeur + il y a N s).
  import { onMount } from "svelte";
  import Modal from "./Modal.svelte";
  import {
    consoEtat,
    consoErreur,
    demarrerPollingConso,
    arreterPollingConso,
  } from "../stores/conso";

  let { onFermer }: { onFermer: () => void } = $props();

  // Polling 1 Hz tant que la modale est montée (arrêt propre au démontage).
  onMount(() => {
    demarrerPollingConso();
    return arreterPollingConso;
  });

  const COULEURS = [
    "var(--accent-violet)",
    "var(--accent-orange)",
    "var(--accent-magenta)",
    "var(--accent-rose)",
    "var(--dash-jaune)",
    "var(--dash-success)",
  ];

  interface Ligne {
    nom: string;
    couleur: string;
    /// Valeurs par échantillon (0 quand la ligne est absente du point).
    serie: number[];
    actuel: number;
    max: number;
    /// RAM en Mo (processus uniquement).
    ram_mo?: number;
    /// GPU % (processus uniquement ; null = non mesurable → « N/A »).
    gpu?: number | null;
  }

  function construireLignes(
    historique: NonNullable<typeof $consoEtat>["historique"],
    champ: "processus" | "threads"
  ): Ligne[] {
    const dernier = historique[historique.length - 1];
    // Ordre = celui du dernier échantillon (déjà trié pertinence côté Rust),
    // puis les noms qui ont disparu (processus/thread mort) à la fin.
    const noms: string[] = dernier ? dernier[champ].map((p) => p.nom) : [];
    for (const e of historique) {
      for (const p of e[champ]) {
        if (!noms.includes(p.nom)) noms.push(p.nom);
      }
    }
    return noms.map((nom, i) => {
      const serie = historique.map(
        (e) => e[champ].find((p) => p.nom === nom)?.cpu_pct ?? 0
      );
      const ram =
        champ === "processus"
          ? dernier?.processus.find((p) => p.nom === nom)?.ram_mo
          : undefined;
      const gpu =
        champ === "processus"
          ? dernier?.processus.find((p) => p.nom === nom)?.gpu_pct
          : undefined;
      return {
        nom,
        couleur: COULEURS[i % COULEURS.length],
        serie,
        actuel: serie[serie.length - 1] ?? 0,
        max: serie.reduce((a, b) => Math.max(a, b), 0),
        ram_mo: ram,
        gpu,
      };
    });
  }

  let lignesProcessus = $derived(
    construireLignes($consoEtat?.historique ?? [], "processus")
  );
  let lignesThreads = $derived(
    construireLignes($consoEtat?.historique ?? [], "threads")
  );

  interface LigneActivite {
    nom: string;
    couleur: string;
    serie: number[];
    actuel: number;
    max: number;
  }

  let lignesActivite = $derived.by((): LigneActivite[] => {
    const h = $consoEtat?.historique ?? [];
    const def = (
      nom: string,
      couleur: string,
      pick: (a: { chat_msgs: number; speedrun_events: number; input_viewer_pubs: number }) => number
    ): LigneActivite => {
      const serie = h.map((e) => pick(e.activite));
      return {
        nom,
        couleur,
        serie,
        actuel: serie[serie.length - 1] ?? 0,
        max: serie.reduce((a, b) => Math.max(a, b), 0),
      };
    };
    return [
      def("Messages chat / s", "var(--accent-violet)", (a) => a.chat_msgs),
      def("Ticks splitter / s", "var(--accent-orange)", (a) => a.speedrun_events),
      def("Pubs input viewer / s", "var(--accent-magenta)", (a) => a.input_viewer_pubs),
    ];
  });

  // ---- Survol : index du point sous le curseur, par nom de ligne ----
  let survol = $state<Record<string, number>>({});

  function onSurvol(e: MouseEvent, nom: string, longueur: number) {
    const svg = e.currentTarget as SVGSVGElement;
    const rect = svg.getBoundingClientRect();
    const ratio = (e.clientX - rect.left) / rect.width;
    survol[nom] = Math.max(
      0,
      Math.min(longueur - 1, Math.round(ratio * (longueur - 1)))
    );
  }

  function delai(idx: number): string {
    const h = $consoEtat?.historique ?? [];
    const ts = h[idx]?.ts;
    const dernier = h[h.length - 1]?.ts;
    if (ts === undefined || dernier === undefined) return "";
    const s = Math.round((dernier - ts) / 1000);
    return s <= 0 ? "maintenant" : `il y a ${s} s`;
  }

  // ---- Rendu ----

  /// Trace SVG (ligne + aire) d'une série. maxEchelle : plafond fixe (100
  /// pour du CPU %) ou null = auto-échelle sur le max de la série.
  function trace(serie: number[], maxEchelle: number | null): { ligne: string; aire: string } {
    const W = 100;
    const H = 36;
    if (serie.length === 0) return { ligne: "", aire: "" };
    const m = Math.max(maxEchelle ?? serie.reduce((a, b) => Math.max(a, b), 0), 1);
    const step = W / Math.max(serie.length - 1, 1);
    const pts = serie.map(
      (v, i) =>
        `${(i * step).toFixed(1)},${(H - (Math.min(v, m) / m) * H).toFixed(1)}`
    );
    const ligne = `M${pts.join(" L")}`;
    const aire = `${ligne} L${W},${H} L0,${H} Z`;
    return { ligne, aire };
  }

  function fmtPct(v: number): string {
    return v < 10 ? `${v.toFixed(1)} %` : `${Math.round(v)} %`;
  }

  function fmtActivite(v: number): string {
    return `${Math.round(v)}`;
  }

  function fmtRam(mo: number | undefined): string {
    if (mo === undefined) return "";
    return mo >= 1024 ? `${(mo / 1024).toFixed(1)} Go` : `${Math.round(mo)} Mo`;
  }
</script>

<Modal
  title="Diagnostic — Consommation"
  onClose={onFermer}
  maxWidth="980px"
  hint="Mesures réelles, rafraîchies chaque seconde. Les courbes couvrent les 5 dernières minutes — survolez pour lire un point précis. Rien n'est enregistré : tout reste en mémoire."
>
  <div class="corps">
    {#if $consoErreur}
      <p class="vide">Mesure indisponible — le backend ne répond pas.</p>
    {:else if ($consoEtat?.historique.length ?? 0) < 2}
      <p class="vide">
        Collecte en cours — les courbes apparaissent dans quelques secondes.
      </p>
    {:else}
      <p class="meta">
        {$consoEtat?.nb_coeurs ?? "?"} cœurs • {$consoEtat?.historique.length ?? 0}
        s d'historique
      </p>

      <!-- ============ VOTRE PC (processus) ============ -->
      <h3>Votre PC</h3>
      <p class="sous-titre">Part du processeur utilisée par chaque programme (% du total).</p>
      {#each lignesProcessus as l (l.nom)}
        {@const t = trace(l.serie, 100)}
        {@const idx = survol[l.nom]}
        <div class="ligne">
          <div class="tete">
            <span class="pastille" style="background:{l.couleur}"></span>
            <span class="nom">{l.nom}</span>
            {#if l.ram_mo !== undefined}
              <span class="ram">{fmtRam(l.ram_mo)}</span>
            {/if}
            {#if l.gpu !== undefined}
              <span class="gpu">{l.gpu === null ? "GPU N/A" : `GPU ${fmtPct(l.gpu)}`}</span>
            {/if}
            <span class="valeur" class:chaud={l.actuel >= 50}>{fmtPct(l.actuel)}</span>
            <span class="max">max {fmtPct(l.max)}</span>
          </div>
          <div class="courbe">
            <svg
              viewBox="0 0 100 36"
              preserveAspectRatio="none"
              role="img"
              aria-label="Courbe {l.nom}"
              onmousemove={(e) => onSurvol(e, l.nom, l.serie.length)}
              onmouseleave={() => delete survol[l.nom]}
            >
              <path d={t.aire} fill={l.couleur} opacity="0.15" />
              <path d={t.ligne} fill="none" stroke={l.couleur} stroke-width="1.5" vector-effect="non-scaling-stroke" />
              {#if idx !== undefined && l.serie[idx] !== undefined}
                {@const x = (idx / Math.max(l.serie.length - 1, 1)) * 100}
                {@const y = 36 - (Math.min(l.serie[idx], 100) / 100) * 36}
                <line x1={x} y1="0" x2={x} y2="36" stroke={l.couleur} stroke-width="0.5" opacity="0.6" vector-effect="non-scaling-stroke" />
                <circle cx={x} cy={y} r="1.8" fill={l.couleur} vector-effect="non-scaling-stroke" />
              {/if}
            </svg>
            {#if idx !== undefined && l.serie[idx] !== undefined}
              <span class="survol">{fmtPct(l.serie[idx])} · {delai(idx)}</span>
            {:else}
              <span class="survol">&nbsp;</span>
            {/if}
          </div>
        </div>
      {/each}

      <!-- ============ DANS STREAMOS (sous-systèmes) ============ -->
      <h3>Dans StreamOS</h3>
      <p class="sous-titre">Charge de chaque fonction (% d'un cœur — 100 % = un cœur saturé).</p>
      {#each lignesThreads as l (l.nom)}
        {@const t = trace(l.serie, 100)}
        {@const idx = survol[l.nom]}
        <div class="ligne">
          <div class="tete">
            <span class="pastille" style="background:{l.couleur}"></span>
            <span class="nom">{l.nom}</span>
            <span class="valeur" class:chaud={l.actuel >= 50}>{fmtPct(l.actuel)}</span>
            <span class="max">max {fmtPct(l.max)}</span>
          </div>
          <div class="courbe">
            <svg
              viewBox="0 0 100 36"
              preserveAspectRatio="none"
              role="img"
              aria-label="Courbe {l.nom}"
              onmousemove={(e) => onSurvol(e, l.nom, l.serie.length)}
              onmouseleave={() => delete survol[l.nom]}
            >
              <path d={t.aire} fill={l.couleur} opacity="0.15" />
              <path d={t.ligne} fill="none" stroke={l.couleur} stroke-width="1.5" vector-effect="non-scaling-stroke" />
              {#if idx !== undefined && l.serie[idx] !== undefined}
                {@const x = (idx / Math.max(l.serie.length - 1, 1)) * 100}
                {@const y = 36 - (Math.min(l.serie[idx], 100) / 100) * 36}
                <line x1={x} y1="0" x2={x} y2="36" stroke={l.couleur} stroke-width="0.5" opacity="0.6" vector-effect="non-scaling-stroke" />
                <circle cx={x} cy={y} r="1.8" fill={l.couleur} vector-effect="non-scaling-stroke" />
              {/if}
            </svg>
            {#if idx !== undefined && l.serie[idx] !== undefined}
              <span class="survol">{fmtPct(l.serie[idx])} · {delai(idx)}</span>
            {:else}
              <span class="survol">&nbsp;</span>
            {/if}
          </div>
        </div>
      {/each}

      <!-- ============ ACTIVITÉ (compteurs /s) ============ -->
      <h3>Activité</h3>
      <p class="sous-titre">Ce que fait l'application en ce moment (par seconde).</p>
      {#each lignesActivite as l (l.nom)}
        {@const t = trace(l.serie, null)}
        {@const idx = survol[l.nom]}
        {@const echelle = Math.max(l.max, 1)}
        <div class="ligne">
          <div class="tete">
            <span class="pastille" style="background:{l.couleur}"></span>
            <span class="nom">{l.nom}</span>
            <span class="valeur">{fmtActivite(l.actuel)}</span>
            <span class="max">max {fmtActivite(l.max)}</span>
          </div>
          <div class="courbe">
            <svg
              viewBox="0 0 100 36"
              preserveAspectRatio="none"
              role="img"
              aria-label="Courbe {l.nom}"
              onmousemove={(e) => onSurvol(e, l.nom, l.serie.length)}
              onmouseleave={() => delete survol[l.nom]}
            >
              <path d={t.aire} fill={l.couleur} opacity="0.15" />
              <path d={t.ligne} fill="none" stroke={l.couleur} stroke-width="1.5" vector-effect="non-scaling-stroke" />
              {#if idx !== undefined && l.serie[idx] !== undefined}
                {@const x = (idx / Math.max(l.serie.length - 1, 1)) * 100}
                {@const y = 36 - (Math.min(l.serie[idx], echelle) / echelle) * 36}
                <line x1={x} y1="0" x2={x} y2="36" stroke={l.couleur} stroke-width="0.5" opacity="0.6" vector-effect="non-scaling-stroke" />
                <circle cx={x} cy={y} r="1.8" fill={l.couleur} vector-effect="non-scaling-stroke" />
              {/if}
            </svg>
            {#if idx !== undefined && l.serie[idx] !== undefined}
              <span class="survol">{fmtActivite(l.serie[idx])}/s · {delai(idx)}</span>
            {:else}
              <span class="survol">&nbsp;</span>
            {/if}
          </div>
        </div>
      {/each}
    {/if}
  </div>
</Modal>

<style>
  .corps {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 0.6rem 1rem 1rem;
  }
  .meta {
    font-size: 0.72rem;
    opacity: 0.6;
    margin: 0.1rem 0 0.6rem;
  }
  h3 {
    font-size: 0.82rem;
    margin: 0.9rem 0 0.1rem;
    color: var(--accent-ui);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  .sous-titre {
    font-size: 0.72rem;
    opacity: 0.65;
    margin: 0 0 0.45rem;
  }
  .vide {
    font-size: 0.85rem;
    opacity: 0.8;
    text-align: center;
    padding: 2rem 0;
  }
  .ligne {
    margin-bottom: 0.55rem;
    background: var(--fond-controle);
    border: 1px solid var(--bordure-ui);
    border-radius: var(--rayon-petit);
    padding: 0.4rem 0.55rem;
  }
  .tete {
    display: flex;
    align-items: center;
    gap: 0.45rem;
    font-size: 0.78rem;
  }
  .pastille {
    width: 9px;
    height: 9px;
    border-radius: 50%;
    flex-shrink: 0;
  }
  .nom {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .ram {
    font-size: 0.7rem;
    opacity: 0.65;
  }
  .gpu {
    font-size: 0.7rem;
    opacity: 0.65;
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
  }
  .valeur {
    font-weight: 700;
    min-width: 3.4rem;
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  .valeur.chaud {
    color: var(--dash-danger);
  }
  .max {
    font-size: 0.68rem;
    opacity: 0.55;
    min-width: 4rem;
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  .courbe {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    margin-top: 0.2rem;
  }
  .courbe svg {
    flex: 1;
    height: 36px;
    display: block;
    cursor: crosshair;
  }
  .survol {
    font-size: 0.68rem;
    opacity: 0.75;
    min-width: 7rem;
    text-align: right;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
</style>
