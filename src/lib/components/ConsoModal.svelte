<script lang="ts">
  // ConsoModal — « Performances » : consommation réelle mesurée par le
  // backend (conso.rs, 1 échantillon/s) : processus (StreamOS, WebView2,
  // OBS, jeu, système) + sous-systèmes (threads nommés : splitter, input
  // viewer…) + compteurs d'activité (chat/s, events speedrun/s, pubs
  // input/s). L'analyse d'audience vit dans StatistiquesModal.svelte.
  //
  // Objectif : ultra lisible pour un non-technicien — une ligne = un nom,
  // sa valeur actuelle, son max récent, et sa courbe des 5 dernières minutes.
  // Le survol d'une courbe affiche le point précis (valeur + il y a N s).
  import { get } from "svelte/store";
  import Modal from "./Modal.svelte";
  import {
    boiteNoire,
    chargerBoiteNoire,
    consoEtat,
    consoErreur,
    demarrerPollingConso,
    arreterPollingConso,
  } from "../stores/conso";
  import type { BoiteNoireEchantillon } from "../tauri";
  import { t } from "../stores/langue";

  let { onFermer }: { onFermer: () => void } = $props();

  // Polling 1 Hz UNIQUEMENT tant que la modale est montée (teardown au
  // démontage).
  $effect(() => {
    demarrerPollingConso();
    // Boîte noire : lecture UNIQUE à l'ouverture (pas de polling — le
    // fichier fait jusqu'à 2 Mio). Elle ne bouge donc pas tant que la
    // modale reste ouverte.
    void chargerBoiteNoire(720);
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
      def(get(t)("consoModal.ligneChat"), "var(--accent-violet)", (a) => a.chat_msgs),
      def(get(t)("consoModal.ligneSplitter"), "var(--accent-orange)", (a) => a.speedrun_events),
      def(get(t)("consoModal.ligneInput"), "var(--accent-magenta)", (a) => a.input_viewer_pubs),
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
    return s <= 0
      ? get(t)("consoModal.maintenant")
      : get(t)("consoModal.ilYA", { s });
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
    return mo >= 1024
      ? `${(mo / 1024).toFixed(1)} ${get(t)("consoModal.go")}`
      : `${Math.round(mo)} ${get(t)("consoModal.mo")}`;
  }

  // ---- Boîte noire (journal persistant, 1 point / 5 s) ----

  /// Points affichés (les plus récents).
  const POINTS_BOITE_NOIRE = 8;

  /// Au-delà de cet âge, le dernier point est considéré comme anormal : la
  /// boucle écrit toutes les 5 s, donc 20 s = 4 échantillons manqués. En
  /// dessous, c'est le fonctionnement normal et l'affichage reste neutre.
  /// (Le rouge n'a de sens qu'après un arrêt : au redémarrage suivant, il
  /// indique l'heure approximative du gel précédent.)
  const SEUIL_AGE_ALERTE = 20;

  let derniersPoints = $derived(
    ($boiteNoire?.echantillons ?? []).slice(-POINTS_BOITE_NOIRE)
  );

  /// Le point le plus récent — porte le compteur d'appels `obs_connect`.
  let dernierPoint = $derived(
    derniersPoints[derniersPoints.length - 1]
  );

  /// Âge du dernier point enregistré. Après un redémarrage, c'est l'heure
  /// approximative du gel précédent — l'information la plus utile ici.
  let ageDernier = $derived.by((): number | null => {
    const e = $boiteNoire?.echantillons ?? [];
    const dernier = e[e.length - 1];
    if (!dernier) return null;
    return Math.max(0, Math.round((Date.now() - dernier.ts) / 1000));
  });

  /// Durée couverte par le journal chargé, en minutes.
  let dureeBoiteNoire = $derived.by((): number => {
    const e = $boiteNoire?.echantillons ?? [];
    if (e.length < 2) return 0;
    return Math.round((e[e.length - 1].ts - e[0].ts) / 60000);
  });

  function heurePoint(ts: number): string {
    return new Date(ts).toLocaleTimeString();
  }

  /// Badge OBS : présence du processus + silence depuis le dernier RPC
  /// réussi (le signal qui manquait le 2026-10-27).
  function libelleObs(p: BoiteNoireEchantillon): string {
    if (!p.obs_present) return "OBS —";
    if (p.obs_silence_s !== null && p.obs_silence_s > 120) {
      return get(t)("consoModal.obsMuet", { s: p.obs_silence_s });
    }
    return "OBS ✓";
  }
</script>

<Modal
  title={$t("consoModal.titre")}
  onClose={onFermer}
  crochet="var(--crochet-teal)"
  maxWidth="61rem"
  hint={$t("consoModal.hint")}
>
  <div class="corps">
    {#if $consoErreur}
      <p class="vide">{$t("consoModal.indisponible")}</p>
    {:else if ($consoEtat?.historique.length ?? 0) < 2}
      <p class="vide">
        {$t("consoModal.collecte")}
      </p>
    {:else}
      <p class="meta">
        {$t("consoModal.meta", {
          coeurs: $consoEtat?.nb_coeurs ?? "?",
          s: $consoEtat?.historique.length ?? 0,
        })}
      </p>

      <!-- ============ VOTRE PC (processus) ============ -->
      <h3>{$t("consoModal.votrePc")}</h3>
      <p class="sous-titre">{$t("consoModal.sousTitrePc")}</p>
      {#each lignesProcessus as l (l.nom)}
        {@const tr = trace(l.serie, 100)}
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
              aria-label={get(t)("consoModal.courbe", { nom: l.nom })}
              onmousemove={(e) => onSurvol(e, l.nom, l.serie.length)}
              onmouseleave={() => delete survol[l.nom]}
            >
              <path d={tr.aire} fill={l.couleur} opacity="0.15" />
              <path d={tr.ligne} fill="none" stroke={l.couleur} stroke-width="1.5" vector-effect="non-scaling-stroke" />
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
      <h3>{$t("consoModal.dansStreamos")}</h3>
      <p class="sous-titre">{$t("consoModal.sousTitreThreads")}</p>
      {#each lignesThreads as l (l.nom)}
        {@const tr = trace(l.serie, 100)}
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
              aria-label={get(t)("consoModal.courbe", { nom: l.nom })}
              onmousemove={(e) => onSurvol(e, l.nom, l.serie.length)}
              onmouseleave={() => delete survol[l.nom]}
            >
              <path d={tr.aire} fill={l.couleur} opacity="0.15" />
              <path d={tr.ligne} fill="none" stroke={l.couleur} stroke-width="1.5" vector-effect="non-scaling-stroke" />
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
      <h3>{$t("consoModal.activite")}</h3>
      <p class="sous-titre">{$t("consoModal.sousTitreActivite")}</p>
      {#each lignesActivite as l (l.nom)}
        {@const tr = trace(l.serie, null)}
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
              aria-label={get(t)("consoModal.courbe", { nom: l.nom })}
              onmousemove={(e) => onSurvol(e, l.nom, l.serie.length)}
              onmouseleave={() => delete survol[l.nom]}
            >
              <path d={tr.aire} fill={l.couleur} opacity="0.15" />
              <path d={tr.ligne} fill="none" stroke={l.couleur} stroke-width="1.5" vector-effect="non-scaling-stroke" />
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

    <!-- ============ BOÎTE NOIRE (journal persistant) ============ -->
    <h3>{$t("consoModal.boiteNoire")}</h3>
    <p class="sous-titre">{$t("consoModal.sousTitreBoiteNoire")}</p>
    {#if derniersPoints.length === 0}
      <p class="vide">{$t("consoModal.boiteNoireVide")}</p>
    {:else}
      <p class="meta">
        {$t("consoModal.boiteNoireMeta", {
          n: $boiteNoire?.echantillons.length ?? 0,
          min: dureeBoiteNoire,
        })}
      </p>
      {#if ageDernier !== null}
        <p class="meta" class:alerte={ageDernier > SEUIL_AGE_ALERTE}>
          {$t("consoModal.boiteNoireAge", { s: ageDernier })}
        </p>
      {/if}
      {#if dernierPoint}
        <p class="meta">
          {$t("consoModal.obsConnect", {
            n: dernierPoint.obs_connect_n,
            origine: dernierPoint.obs_connect_origine ?? "—",
          })}
        </p>
      {/if}
      <div class="boite">
        <div class="point entete">
          <span class="heure">{$t("consoModal.colHeure")}</span>
          <span class="chiffre">{$t("consoModal.colRamApp")}</span>
          <span class="chiffre">{$t("consoModal.colCpuApp")}</span>
          <span class="chiffre">{$t("consoModal.colRamSysteme")}</span>
          <span class="obs">&nbsp;</span>
        </div>
        {#each derniersPoints as p (p.ts)}
          <div class="point">
            <span class="heure">{heurePoint(p.ts)}</span>
            <span class="chiffre">{fmtRam(p.ram_mo)}</span>
            <span class="chiffre">{fmtPct(p.cpu_pct)}</span>
            <span class="chiffre">{fmtRam(p.ram_systeme_mo)}</span>
            <span
              class="obs"
              class:muet={p.obs_present && (p.obs_silence_s ?? 0) > 120}
            >
              {libelleObs(p)}
            </span>
          </div>
        {/each}
      </div>
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
    width: 0.5625rem;
    height: 0.5625rem;
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
    height: 2.25rem;
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
  .alerte {
    color: var(--dash-danger);
    opacity: 0.95;
  }
  .boite {
    margin-top: 0.35rem;
    background: var(--fond-controle);
    border: 1px solid var(--bordure-ui);
    border-radius: var(--rayon-petit);
    padding: 0.35rem 0.55rem;
    font-size: 0.72rem;
    font-variant-numeric: tabular-nums;
  }
  .point {
    display: flex;
    gap: 0.5rem;
    padding: 0.12rem 0;
  }
  .point.entete {
    font-size: 0.66rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    opacity: 0.55;
    border-bottom: 1px solid var(--bordure-ui);
    padding-bottom: 0.2rem;
    margin-bottom: 0.15rem;
  }
  .heure {
    opacity: 0.6;
    min-width: 5rem;
  }
  .chiffre {
    min-width: 4.2rem;
    text-align: right;
  }
  .obs {
    flex: 1;
    text-align: right;
    opacity: 0.75;
    white-space: nowrap;
  }
  .obs.muet {
    color: var(--dash-danger);
    opacity: 1;
  }
</style>
