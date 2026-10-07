<script lang="ts">
  // AudienceTab — onglet « Audience » de la modale Diagnostic : analyse des
  // sessions live Twitch reconstruites depuis telemetrie/events.jsonl
  // (commandes audience_sessions / audience_session — analyse.rs).
  //
  // Layout : en-tête (sélecteurs période + live) → 2 colonnes (graphe ~65 %
  // | cartes agrégats ~35 %) → liste des pics sous le graphe.
  // Graphe SVG inline (aucune dépendance chart — recette ConsoModal) :
  // aire+ligne viewers, bandes jeu alternées, zones pic surlignées, marqueurs
  // timeline (raids/subs/bits/follows/stream-info), survol = point précis.
  import { onMount } from "svelte";
  import { get } from "svelte/store";
  import { t } from "../stores/langue";
  import {
    audienceData,
    audienceDetail,
    audienceErreur,
    audienceChargement,
    audienceChargementDetail,
    audiencePeriode,
    audienceSessionId,
    audiencePicSel,
    chargerAudience,
    chargerSession,
  } from "../stores/audience";
  import type { AudiencePic } from "../tauri";

  onMount(() => {
    void chargerAudience(get(audiencePeriode));
  });

  const PERIODES = [7, 30, 90];

  // ---- Formatage ----

  function fmtHeure(ms: number): string {
    return new Date(ms).toLocaleTimeString(undefined, {
      hour: "2-digit",
      minute: "2-digit",
    });
  }

  function fmtDate(ms: number): string {
    return new Date(ms).toLocaleDateString(undefined, {
      day: "numeric",
      month: "short",
    });
  }

  function fmtDuree(s: number): string {
    const h = Math.floor(s / 3600);
    const m = Math.round((s % 3600) / 60);
    return h > 0 ? `${h} h ${String(m).padStart(2, "0")}` : `${m} min`;
  }

  function labelSession(s: {
    debut: number;
    fin: number;
    duree_s: number;
    pic: number;
  }): string {
    return `${fmtDate(s.debut)} ${fmtHeure(s.debut)}–${fmtHeure(s.fin)} · ${fmtDuree(s.duree_s)} · ${get(t)("audience.sessionPic", { n: s.pic })}`;
  }

  function causeLabel(p: AudiencePic): string {
    const tt = get(t);
    switch (p.cause) {
      case "raid":
        return tt("audience.causeRaid", {
          pseudo: p.cause_pseudo,
          nb: p.cause_nb,
        });
      case "contenu":
        return tt("audience.causeContenu");
      case "communaute":
        return tt("audience.causeCommunaute", { nb: p.cause_nb });
      case "debut_live":
        return tt("audience.causeDebutLive");
      default:
        return tt("audience.causeOrganique");
    }
  }

  function confianceLabel(c: string): string {
    return get(t)(
      c === "certaine"
        ? "audience.confCertaine"
        : c === "probable"
          ? "audience.confProbable"
          : "audience.confFaible"
    );
  }

  function nomJour(j: number): string {
    const jours = get(t)("audience.jours").split("|");
    return jours[j] ?? "";
  }

  // ---- Graphe SVG ----

  const W = 600;
  const H = 160;

  let t0 = $derived($audienceDetail?.serie[0]?.ts ?? 0);
  let t1 = $derived(
    $audienceDetail?.serie[($audienceDetail?.serie.length ?? 1) - 1]?.ts ?? 1
  );

  // Échelle Y « nice » (1/2/5 × 10^n) : la courbe a toujours du headroom et
  // les ticks tombent sur des valeurs rondes.
  function niceMax(v: number): number {
    if (v <= 5) return 5;
    const p = Math.pow(10, Math.floor(Math.log10(v)));
    const u = v / p;
    return (u <= 1 ? 1 : u <= 2 ? 2 : u <= 5 ? 5 : 10) * p;
  }

  let maxV = $derived(
    niceMax(
      Math.max(5, ...($audienceDetail?.serie.map((p) => p.count) ?? [5]))
    )
  );

  function px(ts: number): number {
    return ((ts - t0) / Math.max(t1 - t0, 1)) * W;
  }
  function py(v: number): number {
    return H - (v / maxV) * (H - 6) - 3;
  }
  function clampY(y: number): number {
    return Math.max(3, Math.min(H - 3, y));
  }

  // Courbe lissée : Catmull-Rom → Bézier cubique. La série est en marches
  // (carry-forward) ; les tangentes sont bornées par les points voisins,
  // sans dépassement vertical (clampY) — le lissage embellit, il n'invente pas.
  function lisse(pts: { x: number; y: number }[]): string {
    if (pts.length === 0) return "";
    if (pts.length < 3) {
      return `M${pts.map((p) => `${p.x.toFixed(2)},${p.y.toFixed(2)}`).join(" L")}`;
    }
    let d = `M${pts[0].x.toFixed(2)},${pts[0].y.toFixed(2)}`;
    for (let i = 0; i < pts.length - 1; i++) {
      const p0 = pts[Math.max(0, i - 1)];
      const p1 = pts[i];
      const p2 = pts[i + 1];
      const p3 = pts[Math.min(pts.length - 1, i + 2)];
      const c1x = p1.x + (p2.x - p0.x) / 6;
      const c1y = clampY(p1.y + (p2.y - p0.y) / 6);
      const c2x = p2.x - (p3.x - p1.x) / 6;
      const c2y = clampY(p2.y - (p3.y - p1.y) / 6);
      d += ` C${c1x.toFixed(2)},${c1y.toFixed(2)} ${c2x.toFixed(2)},${c2y.toFixed(2)} ${p2.x.toFixed(2)},${p2.y.toFixed(2)}`;
    }
    return d;
  }

  let chemin = $derived.by(() => {
    const s = $audienceDetail?.serie ?? [];
    if (s.length === 0) return { ligne: "", aire: "" };
    const pts = s.map((p) => ({ x: px(p.ts), y: py(p.count) }));
    const ligne = lisse(pts);
    const aire = `${ligne} L${W},${H} L0,${H} Z`;
    return { ligne, aire };
  });

  // Ticks Y : 0 → maxV en 4 crans, dédoublonnés. Ticks X : 5 heures
  // régulièrement espacées entre début et fin du live.
  let ticksY = $derived(
    [...new Set([0, 0.25, 0.5, 0.75, 1].map((m) => Math.round(m * maxV)))]
  );
  let ticksX = $derived(
    t1 <= t0 ? [] : [0, 1, 2, 3, 4].map((i) => t0 + ((t1 - t0) * i) / 4)
  );

  let survol = $state<number | null>(null);
  let survolPt = $derived(
    survol !== null ? ($audienceDetail?.serie[survol] ?? null) : null
  );

  function onSurvol(e: MouseEvent) {
    const el = e.currentTarget as HTMLElement;
    const r = el.getBoundingClientRect();
    const s = $audienceDetail?.serie ?? [];
    if (s.length < 2) return;
    const ratio = (e.clientX - r.left) / r.width;
    survol = Math.max(
      0,
      Math.min(s.length - 1, Math.round(ratio * (s.length - 1)))
    );
  }

  // Couleur des marqueurs timeline par genre.
  const COULEURS_EVT: Record<string, string> = {
    raid: "var(--accent-magenta)",
    sub: "var(--accent-violet)",
    resub: "var(--accent-violet)",
    subgift: "var(--accent-violet)",
    bits: "var(--dash-jaune)",
    follow: "var(--dash-success)",
    gift: "var(--accent-rose)",
    superchat: "var(--accent-rose)",
    "stream-info": "var(--accent-ui)",
  };

  function couleurEvt(genre: string): string {
    return COULEURS_EVT[genre] ?? "var(--texte-dim, #888)";
  }

  function detailEvt(genre: string, detail: string, nb: number): string {
    if (genre === "stream-info") {
      return detail || get(t)("audience.jeuInconnu");
    }
    return nb > 0 ? `${detail} · ${nb}` : detail;
  }
</script>

<div class="audience">
  <div class="entete">
    <label>
      {$t("audience.periode")}
      <select
        value={$audiencePeriode}
        onchange={(e) =>
          void chargerAudience(
            Number((e.target as HTMLSelectElement).value)
          )}
      >
        {#each PERIODES as j}
          <option value={j}>{$t(`audience.j${j}`)}</option>
        {/each}
      </select>
    </label>
    <label class="sel-session">
      {$t("audience.session")}
      <select
        value={$audienceSessionId ?? ""}
        onchange={(e) =>
          void chargerSession((e.target as HTMLSelectElement).value)}
        disabled={($audienceData?.sessions.length ?? 0) === 0}
      >
        {#each $audienceData?.sessions ?? [] as s (s.session_id)}
          <option value={s.session_id}>{labelSession(s)}</option>
        {/each}
      </select>
    </label>
    <span class="twitch-only">{$t("audience.twitchSeul")}</span>
  </div>

  {#if $audienceErreur}
    <p class="vide">{$t("audience.erreur")}</p>
  {:else if $audienceChargement}
    <p class="vide">{$t("audience.chargement")}</p>
  {:else if ($audienceData?.sessions.length ?? 0) === 0}
    <p class="vide">{$t("audience.aucune")}</p>
    <p class="vide sous-vide">{$t("audience.aucuneDonnee")}</p>
  {:else}
    <div class="layout">
      <!-- ==================== GAUCHE : graphe + pics ==================== -->
      <div class="gauche">
        {#if $audienceDetail}
          <p class="meta">
            {$t("audience.liveMeta", {
              duree: fmtDuree($audienceDetail.meta.duree_s),
              moyenne: Math.round($audienceDetail.meta.moyenne),
              pic: $audienceDetail.meta.pic,
              msgs: $audienceDetail.meta.nb_msgs_chat,
            })}
          </p>
          <div class="graphe">
            <!-- Axe Y : labels HTML hors du SVG (preserveAspectRatio="none"
                 déformerait le texte). Hauteur calée sur celle du plot. -->
            <div class="yaxis" aria-hidden="true">
              {#each ticksY as v (v)}
                <span class="ytick" style="top:{(py(v) / H) * 100}%">{v}</span>
              {/each}
            </div>
            <div class="droite">
              <div class="plot">
                <!-- Bandes jeu -->
                {#each $audienceDetail.segments as seg, i (seg.debut)}
                  <div
                    class="bande"
                    class:alt={i % 2 === 1}
                    style="left:{(px(seg.debut) / W) * 100}%;width:{((px(seg.fin) - px(seg.debut)) / W) * 100}%"
                  >
                    <span class="bande-label"
                      >{seg.jeu || $t("audience.jeuInconnu")}</span
                    >
                  </div>
                {/each}
                <!-- Grille horizontale + moyenne du live -->
                {#each ticksY as v (v)}
                  {#if v > 0}
                    <div class="grid" style="top:{(py(v) / H) * 100}%"></div>
                  {/if}
                {/each}
                <div
                  class="moyenne"
                  style="top:{(py($audienceDetail.meta.moyenne) / H) * 100}%"
                >
                  <span class="moyenne-lbl"
                    >{Math.round($audienceDetail.meta.moyenne)}</span
                  >
                </div>
                <svg
                  viewBox="0 0 {W} {H}"
                  preserveAspectRatio="none"
                  role="img"
                  aria-label={$t("audience.ariaGraphe")}
                  onmousemove={onSurvol}
                  onmouseleave={() => (survol = null)}
                >
                  <defs>
                    <linearGradient id="audAire" x1="0" y1="0" x2="0" y2="1">
                      <stop
                        offset="0"
                        stop-color="var(--accent-violet)"
                        stop-opacity="0.38"
                      />
                      <stop
                        offset="1"
                        stop-color="var(--accent-violet)"
                        stop-opacity="0.02"
                      />
                    </linearGradient>
                    <linearGradient id="audPic" x1="0" y1="0" x2="0" y2="1">
                      <stop
                        offset="0"
                        stop-color="var(--accent-orange)"
                        stop-opacity="0.28"
                      />
                      <stop
                        offset="1"
                        stop-color="var(--accent-orange)"
                        stop-opacity="0.04"
                      />
                    </linearGradient>
                  </defs>
                  {#each $audienceDetail.pics as p, i (p.t_debut)}
                    <rect
                      x={px(p.t_debut)}
                      y="0"
                      width={Math.max(px(p.t_fin) - px(p.t_debut), 0.5)}
                      height={H}
                      fill="url(#audPic)"
                      opacity={$audiencePicSel === i ? 1 : 0.45}
                    />
                  {/each}
                  <path d={chemin.aire} fill="url(#audAire)" />
                  <path
                    d={chemin.ligne}
                    fill="none"
                    stroke="var(--accent-violet)"
                    stroke-width="4"
                    opacity="0.18"
                    stroke-linecap="round"
                    vector-effect="non-scaling-stroke"
                  />
                  <path
                    d={chemin.ligne}
                    fill="none"
                    stroke="var(--accent-violet)"
                    stroke-width="1.5"
                    stroke-linejoin="round"
                    vector-effect="non-scaling-stroke"
                  />
                  {#if $audiencePicSel !== null && $audienceDetail.pics[$audiencePicSel]}
                    <line
                      x1={px($audienceDetail.pics[$audiencePicSel].t_pic)}
                      y1="0"
                      x2={px($audienceDetail.pics[$audiencePicSel].t_pic)}
                      y2={H}
                      stroke="var(--accent-orange)"
                      stroke-width="1"
                      stroke-dasharray="3 2"
                      vector-effect="non-scaling-stroke"
                    />
                  {/if}
                  {#if survolPt}
                    <line
                      x1={px(survolPt.ts)}
                      y1="0"
                      x2={px(survolPt.ts)}
                      y2={H}
                      stroke="var(--accent-violet)"
                      stroke-width="0.5"
                      opacity="0.6"
                      vector-effect="non-scaling-stroke"
                    />
                    <circle
                      cx={px(survolPt.ts)}
                      cy={py(survolPt.count)}
                      r="4.5"
                      fill="var(--accent-violet)"
                      opacity="0.25"
                      vector-effect="non-scaling-stroke"
                    />
                    <circle
                      cx={px(survolPt.ts)}
                      cy={py(survolPt.count)}
                      r="2"
                      fill="var(--accent-violet)"
                      vector-effect="non-scaling-stroke"
                    />
                  {/if}
                </svg>
                <!-- Marqueurs timeline (events + stream-info) -->
                <div class="marqueurs">
                  {#each $audienceDetail.timeline as e (e.ts)}
                    <span
                      class="marqueur"
                      style="left:{(px(e.ts) / W) * 100}%;background:{couleurEvt(e.genre)}"
                      title={`${fmtHeure(e.ts)} · ${e.genre} · ${detailEvt(e.genre, e.detail, e.nb)}`}
                    ></span>
                  {/each}
                </div>
                {#if survolPt}
                  <div
                    class="tooltip"
                    class:bas={py(survolPt.count) < 26}
                    style="left:{(px(survolPt.ts) / W) * 100}%;top:{(py(survolPt.count) / H) * 100}%"
                  >
                    {$t("audience.survol", {
                      count: survolPt.count,
                      heure: fmtHeure(survolPt.ts),
                    })}
                  </div>
                {/if}
              </div>
              <div class="xaxis" aria-hidden="true">
                {#each ticksX as ts, i (ts)}
                  <span
                    class="xtick"
                    style="left:{(px(ts) / W) * 100}%;transform:translateX({i === 0 ? 0 : i === ticksX.length - 1 ? -100 : -50}%)"
                    >{fmtHeure(ts)}</span
                  >
                {/each}
              </div>
            </div>
          </div>

          <h3>{$t("audience.pics")}</h3>
          {#if $audienceDetail.pics.length === 0}
            <p class="sous-titre">{$t("audience.aucunPic")}</p>
          {:else}
            {#each $audienceDetail.pics as p, i (p.t_debut)}
              <button
                class="pic"
                class:sel={$audiencePicSel === i}
                onclick={() => audiencePicSel.set($audiencePicSel === i ? null : i)}
              >
                <span class="pic-heure">{fmtHeure(p.t_pic)}</span>
                <span class="pic-delta">+{p.delta}</span>
                <span class="pic-cause">{causeLabel(p)}</span>
                <span
                  class="pic-conf"
                  class:certaine={p.confiance === "certaine"}
                  class:faible={p.confiance === "faible"}
                  >{confianceLabel(p.confiance)}</span
                >
              </button>
            {/each}
          {/if}
          <p class="resolution">{$t("audience.resolution")}</p>
        {:else if $audienceChargementDetail}
          <p class="vide">{$t("audience.chargement")}</p>
        {/if}
      </div>

      <!-- ==================== DROITE : cartes agrégats ==================== -->
      <div class="cartes">
        {#if $audienceData?.agregats}
          {@const agg = $audienceData.agregats}
          {@const top = agg.top_pics[0]}
          {#if top}
            <div class="carte">
              <h4>{$t("audience.topPic")}</h4>
              <p class="carte-val">
                {$t("audience.topPicLigne", {
                  delta: top.delta,
                  heure: fmtHeure(top.t_pic),
                  cause: causeLabel(top),
                })}
              </p>
              <p class="carte-sub">{confianceLabel(top.confiance)}</p>
            </div>
          {/if}
          {@const jeu = agg.top_jeux[0]}
          {#if jeu}
            <div class="carte">
              <h4>{$t("audience.meilleureCategorie")}</h4>
              <p class="carte-val">{jeu.jeu || $t("audience.jeuInconnu")}</p>
              <p class="carte-sub">
                {$t("audience.carteJeu", {
                  moyenne: Math.round(jeu.moyenne),
                  temps: fmtDuree(jeu.temps_s),
                  pics: jeu.nb_pics,
                })}
              </p>
            </div>
          {/if}
          {@const titre = agg.top_titres[0]}
          {#if titre && titre.titre}
            <div class="carte">
              <h4>{$t("audience.meilleurTitre")}</h4>
              <p class="carte-val ellipsis">{titre.titre}</p>
              <p class="carte-sub">
                {$t("audience.carteMoyenne", {
                  moyenne: Math.round(titre.moyenne),
                })}
              </p>
            </div>
          {/if}
          {@const cr = agg.top_creneaux[0]}
          {#if cr}
            <div class="carte">
              <h4>{$t("audience.meilleurCreneau")}</h4>
              <p class="carte-val">
                {$t("audience.creneauLigne", {
                  jour: nomJour(cr.jour),
                  heure: cr.heure,
                  moyenne: Math.round(cr.moyenne),
                })}
              </p>
            </div>
          {/if}
          <div class="carte">
            <h4>{$t("audience.pendantPeriode")}</h4>
            <p class="carte-val">
              {$t("audience.totaux", {
                raids: agg.totaux.raids,
                subs: agg.totaux.subs + agg.totaux.resubs + agg.totaux.subgifts,
                bits: agg.totaux.bits_total,
                follows: agg.totaux.follows,
              })}
            </p>
          </div>
        {/if}
      </div>
    </div>
  {/if}
</div>

<style>
  .audience {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 0.6rem 1rem 1rem;
    display: flex;
    flex-direction: column;
  }
  .entete {
    display: flex;
    align-items: center;
    gap: 0.8rem;
    flex-wrap: wrap;
    margin-bottom: 0.6rem;
    font-size: 0.78rem;
  }
  .entete label {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    opacity: 0.85;
  }
  .entete select {
    background: var(--fond-controle);
    color: var(--texte);
    border: 1px solid var(--bordure-ui);
    border-radius: var(--rayon-petit);
    padding: 0.2rem 0.4rem;
    font-size: 0.78rem;
    max-width: 22rem;
  }
  .twitch-only {
    font-size: 0.68rem;
    opacity: 0.55;
    margin-left: auto;
  }
  .layout {
    display: grid;
    grid-template-columns: 2fr 1fr;
    gap: 0.8rem;
    align-items: start;
  }
  .meta {
    font-size: 0.72rem;
    opacity: 0.6;
    margin: 0 0 0.4rem;
  }
  .graphe {
    display: flex;
    align-items: flex-start;
    background: var(--fond-controle);
    border: 1px solid var(--bordure-ui);
    border-radius: var(--rayon-petit);
    overflow: hidden;
    padding: 0.35rem 0.35rem 0;
  }
  .yaxis {
    position: relative;
    width: 1.8rem;
    flex: none;
    height: 180px;
  }
  .ytick {
    position: absolute;
    right: 4px;
    transform: translateY(-50%);
    font-size: 0.6rem;
    opacity: 0.55;
    font-variant-numeric: tabular-nums;
  }
  .droite {
    flex: 1;
    min-width: 0;
  }
  .plot {
    position: relative;
  }
  .grid {
    position: absolute;
    left: 0;
    right: 0;
    border-top: 1px solid var(--bordure-ui);
    opacity: 0.45;
    pointer-events: none;
  }
  .moyenne {
    position: absolute;
    left: 0;
    right: 0;
    border-top: 1px dashed var(--accent-violet);
    opacity: 0.4;
    pointer-events: none;
  }
  .moyenne-lbl {
    position: absolute;
    right: 4px;
    transform: translateY(-110%);
    font-size: 0.6rem;
    color: var(--accent-violet);
    opacity: 0.85;
    font-variant-numeric: tabular-nums;
  }
  .xaxis {
    position: relative;
    height: 15px;
  }
  .xtick {
    position: absolute;
    top: 2px;
    font-size: 0.6rem;
    opacity: 0.55;
    font-variant-numeric: tabular-nums;
  }
  .bande {
    position: absolute;
    top: 0;
    bottom: 0;
    background: var(--accent-ui);
    opacity: 0.05;
    overflow: hidden;
    pointer-events: none;
  }
  .bande.alt {
    opacity: 0.1;
  }
  .bande-label {
    position: absolute;
    top: 2px;
    left: 4px;
    font-size: 0.62rem;
    color: var(--texte);
    opacity: 0.7;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 100%;
  }
  .plot svg {
    position: relative;
    display: block;
    width: 100%;
    height: 180px;
    cursor: crosshair;
  }
  .marqueurs {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 4px;
    height: 8px;
    pointer-events: none;
  }
  .marqueur {
    position: absolute;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    transform: translateX(-50%);
    pointer-events: auto;
  }
  /* Tige reliant le point au bas du graphe */
  .marqueur::before {
    content: "";
    position: absolute;
    left: 50%;
    bottom: 5px;
    width: 1px;
    height: 10px;
    background: inherit;
    opacity: 0.55;
    transform: translateX(-50%);
  }
  .tooltip {
    position: absolute;
    transform: translate(-50%, -125%);
    background: var(--fond-controle);
    border: 1px solid var(--bordure-ui-active);
    border-radius: var(--rayon-petit);
    padding: 0.15rem 0.45rem;
    font-size: 0.66rem;
    white-space: nowrap;
    pointer-events: none;
    font-variant-numeric: tabular-nums;
    z-index: 2;
  }
  /* Point proche du sommet : le tooltip bascule sous le point */
  .tooltip.bas {
    transform: translate(-50%, 25%);
  }
  h3 {
    font-size: 0.82rem;
    margin: 0.7rem 0 0.2rem;
    color: var(--accent-ui);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  .sous-titre,
  .resolution {
    font-size: 0.72rem;
    opacity: 0.6;
    margin: 0.2rem 0;
  }
  .pic {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    width: 100%;
    background: var(--fond-controle);
    border: 1px solid var(--bordure-ui);
    border-radius: var(--rayon-petit);
    color: var(--texte);
    padding: 0.35rem 0.55rem;
    margin-bottom: 0.3rem;
    cursor: pointer;
    font-size: 0.78rem;
    text-align: left;
  }
  .pic:hover {
    border-color: var(--bordure-ui-active);
  }
  .pic.sel {
    border-color: var(--accent-orange);
  }
  .pic-heure {
    min-width: 3rem;
    font-variant-numeric: tabular-nums;
    opacity: 0.8;
  }
  .pic-delta {
    min-width: 3rem;
    font-weight: 700;
    color: var(--accent-orange);
    font-variant-numeric: tabular-nums;
  }
  .pic-cause {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .pic-conf {
    font-size: 0.66rem;
    opacity: 0.6;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .pic-conf.certaine {
    color: var(--dash-success);
    opacity: 1;
  }
  .pic-conf.faible {
    opacity: 0.4;
  }
  .cartes {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }
  .carte {
    background: var(--fond-controle);
    border: 1px solid var(--bordure-ui);
    border-radius: var(--rayon-petit);
    padding: 0.5rem 0.65rem;
  }
  .carte h4 {
    margin: 0 0 0.2rem;
    font-size: 0.68rem;
    color: var(--accent-ui);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  .carte-val {
    margin: 0;
    font-size: 0.82rem;
    font-weight: 600;
  }
  .carte-val.ellipsis {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .carte-sub {
    margin: 0.15rem 0 0;
    font-size: 0.7rem;
    opacity: 0.6;
  }
  .vide {
    font-size: 0.85rem;
    opacity: 0.8;
    text-align: center;
    padding: 1.5rem 0 0;
  }
  .sous-vide {
    padding-top: 0.2rem;
    font-size: 0.72rem;
    opacity: 0.55;
  }
  @media (max-width: 820px) {
    .layout {
      grid-template-columns: 1fr;
    }
  }
</style>
