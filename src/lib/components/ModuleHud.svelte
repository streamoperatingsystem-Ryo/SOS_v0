
<script lang="ts">
  // Module HUD sur le canvas du dashboard (horloge / musique en cours /
  // objectifs du stream). Couche SÉPARÉE des widgets : pas de sélection
  // widget, pas de titres — un rectangle libre drag/resize (8 poignées)
  // avec guides d'alignement + snap magnétique comme les widgets
  // (stores/guides + scene.ts), position commitée au pointerup (setModule).
  // Le rendu reprend celui de diffusion.html (.module.*) pour un aperçu
  // fidèle ; les valeurs live viennent des stores existants (valeursGoals,
  // goalsTwitch, musiqueEtat) et d'une horloge locale 1 s.
  // Clic sans déplacement → ouvre la modale Modules sur ce module.
  import { onMount } from "svelte";
  import { get } from "svelte/store";
  import {
    GOAL_METRIQUES,
    type ModuleId,
    type ModuleHud,
    type LigneObjectif,
    type CadreConfig,
  } from "../tauri";
  import {
    sceneStore,
    setModule,
    buildSnapTargetsModule,
    computeSnap,
    type SnapTarget,
  } from "../stores/scene";
  import { afficherGuidesDrag, masquerGuides } from "../stores/guides";
  import { valeursGoals, goalsTwitch } from "../stores/objectifs";
  import { musiqueEtat, initMusique } from "../stores/musique";
  import { modulesModalOpen } from "../stores/ui";
  import { t, localeTag } from "../stores/langue";
  import { hexValide } from "../cadres/couleurs";

  let { id, m, scale }: { id: ModuleId; m: ModuleHud; scale: number } = $props();

  // ===== Accent = extrémité la plus lumineuse du gradient du cadre actif =====
  // Même résolution que bpmResoudreCouleurs (diffusion.html) : le cadre
  // défaut est noir→violet, prendre `couleur` donnerait un accent noir.
  function luminance(hex: string): number {
    const n = parseInt(hex.slice(1), 16);
    return 0.299 * ((n >> 16) & 0xff) + 0.587 * ((n >> 8) & 0xff) + 0.114 * (n & 0xff);
  }
  function accentCadre(cadre: CadreConfig | undefined): string {
    if (!cadre || !cadre.actif) return "#e0e0e0";
    const c1 = hexValide(cadre.couleur) ? cadre.couleur : null;
    const c2 = hexValide(cadre.couleurFin) ? cadre.couleurFin : null;
    if (c1 && c2) return luminance(c1) < luminance(c2) ? c2 : c1;
    return c1 ?? "#e0e0e0";
  }
  let accent = $derived(accentCadre($sceneStore.cadreWidget));

  // ===== Position locale (fluide pendant le drag, commit au pointerup) =====
  let lx = $state(0);
  let ly = $state(0);
  let lw = $state(0);
  let lh = $state(0);
  let dragging = $state(false);
  let resizing = $state(false);
  $effect(() => {
    if (!dragging && !resizing) {
      lx = m.x;
      ly = m.y;
      lw = m.largeur;
      lh = m.hauteur;
    }
  });
  const MIN_W = 80;
  const MIN_H = 36;
  let handle = "";
  let startX = 0, startY = 0, startWX = 0, startWY = 0, startW = 0, startH = 0;
  let aBouge = false;
  let outerEl: HTMLDivElement;
  // Cibles de snap figées au pointerdown (snapshot — pas de get() par frame).
  let snapTargets: { xs: SnapTarget[]; ys: SnapTarget[] } | null = null;

  function clampCanvas() {
    const cw = $sceneStore.canvasW ?? 1920;
    const ch = $sceneStore.canvasH ?? 1080;
    lw = Math.max(MIN_W, Math.min(cw, lw));
    lh = Math.max(MIN_H, Math.min(ch, lh));
    lx = Math.max(0, Math.min(cw - lw, lx));
    ly = Math.max(0, Math.min(ch - lh, ly));
  }

  function onDown(e: PointerEvent) {
    if (e.button !== 0) return;
    e.stopPropagation();
    e.preventDefault();
    dragging = true;
    aBouge = false;
    startX = e.clientX;
    startY = e.clientY;
    startWX = lx;
    startWY = ly;
    // Cibles de snap figées + guides d'alignement (même mécanique que Widget).
    snapTargets = buildSnapTargetsModule(get(sceneStore), id);
    afficherGuidesDrag({ x: lx, y: ly, width: lw, height: lh });
    outerEl.setPointerCapture(e.pointerId);
  }

  function onHandleDown(e: PointerEvent, h: string) {
    if (e.button !== 0) return;
    e.stopPropagation();
    e.preventDefault();
    resizing = true;
    handle = h;
    aBouge = true;
    startX = e.clientX;
    startY = e.clientY;
    startWX = lx;
    startWY = ly;
    startW = lw;
    startH = lh;
    snapTargets = buildSnapTargetsModule(get(sceneStore), id);
    afficherGuidesDrag({ x: lx, y: ly, width: lw, height: lh });
    outerEl.setPointerCapture(e.pointerId);
  }

  function onMove(e: PointerEvent) {
    const dx = (e.clientX - startX) / scale;
    const dy = (e.clientY - startY) / scale;
    if (dragging) {
      if (Math.abs(dx) > 1 || Math.abs(dy) > 1) aBouge = true;
      let nx = startWX + dx;
      let ny = startWY + dy;
      // Grille magnétique sauf si Alt enfoncé (free-drag) — comme Widget.
      if (!e.altKey && snapTargets) {
        const snap = computeSnap(nx, ny, lw, lh, snapTargets);
        nx = snap.x;
        ny = snap.y;
      }
      lx = nx;
      ly = ny;
      clampCanvas();
      afficherGuidesDrag({ x: lx, y: ly, width: lw, height: lh });
    } else if (resizing) {
      let nx = startWX, ny = startWY, nw = startW, nh = startH;
      if (handle.includes("e")) nw = startW + dx;
      if (handle.includes("s")) nh = startH + dy;
      if (handle.includes("w")) { nw = Math.max(MIN_W, startW - dx); nx = startWX + (startW - nw); }
      if (handle.includes("n")) { nh = Math.max(MIN_H, startH - dy); ny = startWY + (startH - nh); }
      // Snap au resize — même math que Widget : le delta décale la
      // position, les poignées w/n le répercutent sur la taille.
      if (!e.altKey && snapTargets) {
        const snap = computeSnap(nx, ny, nw, nh, snapTargets);
        const ddx = snap.x - nx;
        const ddy = snap.y - ny;
        nx = snap.x;
        ny = snap.y;
        if (handle.includes("w")) nw -= ddx;
        if (handle.includes("n")) nh -= ddy;
      }
      lx = nx; ly = ny; lw = nw; lh = nh;
      clampCanvas();
      afficherGuidesDrag({ x: lx, y: ly, width: lw, height: lh });
    }
  }

  async function onUp() {
    if (!dragging && !resizing) return;
    const etaitClic = dragging && !aBouge;
    dragging = false;
    resizing = false;
    snapTargets = null;
    masquerGuides();
    if (etaitClic) {
      modulesModalOpen.set(id);
      return;
    }
    await setModule(id, {
      x: Math.round(lx),
      y: Math.round(ly),
      largeur: Math.round(lw),
      hauteur: Math.round(lh),
    });
  }

  // ===== Style commun (miroir appliquerStyleModule — diffusion.html) =====
  let fsDefaut = $derived.by(() => {
    if (id === "horloge") return lh * 0.42;
    if (id === "musique") return lh * 0.24;
    if (id === "equaliseur") return lh * 0.14;
    const n = Math.max(1, m.lignes?.length ?? 0);
    return Math.min(lh / (n * 2.4 + (m.titre ? 1.6 : 0.8)), lw * 0.075);
  });
  let fs = $derived(Math.max(8, Math.round(m.taille || fsDefaut)));
  let font = $derived(m.police ? `'${m.police}', 'Segoe UI', sans-serif` : "'Segoe UI', system-ui, sans-serif");
  let opacite = $derived(typeof m.opacite === "number" ? Math.min(1, Math.max(0, m.opacite)) : 0.45);

  // ===== Horloge (locale 1 s) =====
  let now = $state(new Date());
  onMount(() => {
    if (id === "musique") void initMusique();
    if (id !== "horloge") return;
    const timer = setInterval(() => (now = new Date()), 1000);
    return () => clearInterval(timer);
  });
  const pad2 = (n: number) => String(n).padStart(2, "0");
  let dateTxt = $derived(`${pad2(now.getDate())} / ${pad2(now.getMonth() + 1)} / ${pad2(now.getFullYear() % 100)}`);
  let heureTxt = $derived.by(() => {
    let h = now.getHours();
    if (m.format12h) h = h % 12 || 12;
    let s = `${pad2(h)} : ${pad2(now.getMinutes())}`;
    if (m.secondes) s += ` : ${pad2(now.getSeconds())}`;
    return s;
  });
  let ampm = $derived(m.format12h ? (now.getHours() >= 12 ? "PM" : "AM") : "");

  // ===== Musique (snapshot progression du store — pas de tick : aperçu) =====
  const MM_NB_POINTS = 5;
  let enCours = $derived($musiqueEtat?.en_cours ?? null);
  let muPct = $derived.by(() => {
    const p = $musiqueEtat?.progression;
    return p && p.duree_s && p.duree_s > 0 ? Math.min(1, p.position_ms / 1000 / p.duree_s) : 0;
  });
  const fmt = (s: number) => (isFinite(s) && s >= 0 ? `${pad2(Math.floor(s / 60))}:${pad2(Math.floor(s % 60))}` : "00:00");
  let muPos = $derived(enCours ? fmt(($musiqueEtat?.progression?.position_ms ?? 0) / 1000) : "00:00");
  let muRest = $derived.by(() => {
    const p = $musiqueEtat?.progression;
    return enCours && p?.duree_s ? fmt(Math.max(0, p.duree_s - p.position_ms / 1000)) : "00:00";
  });

  // ===== Objectifs (stores valeursGoals / goalsTwitch) =====
  function ligneValeurs(l: LigneObjectif): { label: string; actuel?: number; cible: number } {
    if (l.goalId) {
      const g = $goalsTwitch[l.goalId];
      const label = l.label || (g ? g.description || $t("goal.ttype." + g.goal_type) : $t("carteEdition.goalsTwitch"));
      return { label, actuel: g?.actuel, cible: g?.cible ?? 0 };
    }
    const plat = l.plateforme ?? "twitch";
    const met = l.metrique ?? GOAL_METRIQUES[plat][0];
    return {
      label: l.label || $t("goal.metrique." + met),
      actuel: $valeursGoals[plat]?.[met],
      cible: l.cible ?? 0,
    };
  }
  function pct(actuel: number | undefined, cible: number): number {
    return cible && typeof actuel === "number" ? Math.min(100, Math.round((actuel / cible) * 100)) : 0;
  }
  const nbTxt = (v: number | undefined) => (typeof v === "number" ? v.toLocaleString(get(localeTag)) : "N/A");
</script>

<div
  bind:this={outerEl}
  class="module {id}"
  data-module-id={id}
  class:actif={dragging || resizing}
  style="left:{lx}px; top:{ly}px; width:{lw}px; height:{lh}px; --mod-fs:{fs}px; --mod-font:{font}; --mod-opacite:{opacite}; --mod-accent:{accent};"
  onpointerdown={onDown}
  onpointermove={onMove}
  onpointerup={onUp}
  role="presentation"
  title={$t("modules.cliquerConfigurer")}
>
  {#if id === "horloge"}
    <div class="mod-fond"></div>
    {#if m.afficherDate !== false}
      <span class="mh-date">{dateTxt}</span>
      <span class="mh-sep"></span>
    {/if}
    <span class="mh-heure">{heureTxt}{#if ampm}<span class="mh-ampm">{ampm}</span>{/if}</span>
  {:else if id === "musique"}
    <div class="mm-carte">
      <div class="mm-icone">♫</div>
      <div class="mm-txt">
        <div class="mm-titre">{enCours ? enCours.titre : $t("modules.musique.rien")}</div>
        <div class="mm-sous">{enCours ? (enCours.demandeur ? $t("modules.musique.demandePar", { pseudo: enCours.demandeur }) : "") : $t("modules.musique.lancer")}</div>
      </div>
    </div>
    {#if m.afficherTemps !== false}
      <div class="mm-temps">
        <span>{muPos}</span>
        <span class="mm-points">
          {#each Array(MM_NB_POINTS) as _, i (i)}
            <i class:on={i < Math.round(muPct * MM_NB_POINTS)}></i>
          {/each}
        </span>
        <span>{muRest}</span>
      </div>
    {/if}
  {:else if id === "equaliseur"}
    <!-- Placeholder : le vrai spectre n'existe qu'en diffusion (sous les
         widgets). Barres figées = aperçu de l'emplacement/gabarit. -->
    <div class="eq-ph">
      {#each [0.3, 0.65, 0.45, 0.85, 0.55, 0.95, 0.4, 0.7, 0.5, 0.8, 0.35, 0.6] as h, i (i)}
        <i style="height:{Math.round(h * 100)}%"></i>
      {/each}
    </div>
    <span class="eq-label">{$t("modules.equaliseur.placeholder")}</span>
  {:else}
    <div class="mod-fond"></div>
    {#if m.titre}<div class="mo-titre">{m.titre}</div>{/if}
    {#if !(m.lignes?.length)}
      <div class="mo-vide">{$t("modules.objectifs.vide")}</div>
    {/if}
    {#each m.lignes ?? [] as l (l.id)}
      {@const v = ligneValeurs(l)}
      <div class="mo-ligne">
        <div class="mo-haut">
          <span class="mo-label">{v.label}</span>
          <span class="mo-val"><span class="mo-actuel">{nbTxt(v.actuel)}</span> / {v.cible ? v.cible.toLocaleString($localeTag) : "—"}</span>
        </div>
        <div class="mo-bar"><div class="mo-fill" style="width:{pct(v.actuel, v.cible)}%"></div></div>
      </div>
    {/each}
  {/if}

  <!-- Poignées resize : 4 coins + 4 bords (visibles au survol / pendant l'action). -->
  {#each ["nw", "n", "ne", "e", "se", "s", "sw", "w"] as h (h)}
    <div class="poignee {h}" onpointerdown={(e) => onHandleDown(e, h)} role="presentation"></div>
  {/each}
</div>

<style>
  /* Miroir des règles .module.* de diffusion.html (aperçu fidèle). La couche
     est au-dessus des widgets (z 9000 côté diffusion) — ici z-index élevé
     dans .canvas, sous le cadre du bord (.cadre-app-overlay, pointer-events
     none) et les guides. */
  .module {
    position: absolute;
    box-sizing: border-box;
    z-index: 8000;
    color: #f4f4f6;
    font-family: var(--mod-font);
    font-size: var(--mod-fs);
    text-shadow: 0 1px 2px rgba(0, 0, 0, 0.6);
    /* visible (≠ diffusion) : les poignées débordent de 5 px. */
    overflow: visible;
    cursor: grab;
    outline: 1px dashed transparent;
    outline-offset: 2px;
    transition: outline-color 120ms ease;
    user-select: none;
  }
  .module:hover, .module.actif { outline-color: var(--mod-accent); }
  .module.actif { cursor: grabbing; }
  .module > * { position: relative; z-index: 1; }
  .mod-fond {
    position: absolute;
    inset: 0;
    z-index: 0;
    border-radius: 10px;
    background: rgba(10, 10, 14, var(--mod-opacite));
  }
  /* Horloge */
  .module.horloge {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 0.7em;
    padding: 0 0.8em;
    white-space: nowrap;
  }
  .mh-date { letter-spacing: 0.25em; font-size: 0.78em; color: #d6d6dc; font-variant-numeric: tabular-nums; }
  .mh-sep {
    width: 2px;
    height: 1.1em;
    border-radius: 2px;
    background: var(--mod-accent);
    box-shadow: 0 0 6px var(--mod-accent);
  }
  .mh-heure { letter-spacing: 0.18em; font-weight: 700; font-variant-numeric: tabular-nums; }
  .mh-ampm { font-size: 0.5em; margin-left: 0.4em; color: #d6d6dc; }
  /* Musique */
  .module.musique { display: flex; align-items: stretch; gap: 0.5em; overflow: visible; }
  .mm-carte, .mm-temps {
    display: flex;
    align-items: center;
    border-radius: 10px;
    background: rgba(10, 10, 14, var(--mod-opacite));
    box-sizing: border-box;
    overflow: hidden;
  }
  .mm-carte { flex: 1; min-width: 0; gap: 0.6em; padding: 0.35em 0.6em 0.35em 0.35em; }
  .mm-icone {
    flex: none;
    width: 2.2em;
    height: 2.2em;
    border-radius: 8px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgba(255, 255, 255, 0.08);
    color: var(--mod-accent);
  }
  .mm-txt { min-width: 0; display: flex; flex-direction: column; gap: 0.15em; }
  .mm-titre { font-weight: 700; font-size: 0.95em; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .mm-sous { font-size: 0.68em; color: #c9c9d0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .mm-temps { flex: none; gap: 0.5em; padding: 0 0.7em; font-size: 0.62em; font-variant-numeric: tabular-nums; letter-spacing: 0.05em; }
  .mm-points { display: flex; gap: 0.3em; }
  .mm-points i { display: block; width: 0.45em; height: 0.45em; border-radius: 50%; background: rgba(255, 255, 255, 0.25); }
  .mm-points i.on { background: var(--mod-accent); box-shadow: 0 0 4px var(--mod-accent); }
  /* Objectifs */
  .module.objectifs { display: flex; flex-direction: column; justify-content: center; gap: 0.55em; padding: 0.7em 0.9em; }
  .mo-titre { font-size: 0.6em; letter-spacing: 0.3em; text-transform: uppercase; color: #b9b9c2; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .mo-ligne { display: flex; flex-direction: column; gap: 0.25em; }
  .mo-haut { display: flex; align-items: baseline; justify-content: space-between; gap: 0.6em; white-space: nowrap; }
  .mo-label { font-weight: 700; font-size: 0.72em; letter-spacing: 0.16em; text-transform: uppercase; overflow: hidden; text-overflow: ellipsis; }
  .mo-val { font-size: 0.72em; letter-spacing: 0.08em; font-variant-numeric: tabular-nums; color: #c9c9d0; }
  .mo-actuel { color: var(--mod-accent); font-weight: 700; }
  .mo-bar { height: 2px; border-radius: 2px; background: rgba(255, 255, 255, 0.22); overflow: hidden; }
  .mo-fill { height: 100%; background: var(--mod-accent); box-shadow: 0 0 6px var(--mod-accent); transition: width 0.5s ease; }
  .mo-vide { font-size: 0.65em; color: #b9b9c2; text-align: center; }
  /* Équaliseur — placeholder dashboard (aperçu statique, le rendu animé
     n'existe qu'en diffusion). Translucide comme l'overlay réel. */
  .module.equaliseur { display: flex; align-items: flex-end; }
  .eq-ph {
    display: flex;
    align-items: flex-end;
    gap: 1.5%;
    width: 100%;
    height: 72%;
    padding: 0 1.2%;
    box-sizing: border-box;
  }
  .eq-ph i {
    flex: 1;
    border-radius: 3px 3px 0 0;
    background: linear-gradient(to top, var(--mod-accent), transparent 130%);
    opacity: 0.75;
    box-shadow: 0 0 8px color-mix(in srgb, var(--mod-accent) 45%, transparent);
  }
  .eq-label {
    position: absolute;
    top: 0.35em;
    left: 50%;
    transform: translateX(-50%);
    font-size: 0.62em;
    letter-spacing: 0.22em;
    text-transform: uppercase;
    color: #b9b9c2;
    white-space: nowrap;
  }
  /* Poignées (même géométrie que Widget.svelte, couleur accent). */
  .poignee {
    position: absolute;
    width: 10px;
    height: 10px;
    background: var(--mod-accent);
    border: 1px solid #111;
    border-radius: 2px;
    opacity: 0;
    transition: opacity 120ms ease;
    z-index: 3;
  }
  .module:hover .poignee, .module.actif .poignee { opacity: 1; }
  .poignee.nw { left: -5px; top: -5px; cursor: nwse-resize; }
  .poignee.n { left: calc(50% - 5px); top: -5px; cursor: ns-resize; }
  .poignee.ne { right: -5px; top: -5px; cursor: nesw-resize; }
  .poignee.e { right: -5px; top: calc(50% - 5px); cursor: ew-resize; }
  .poignee.se { right: -5px; bottom: -5px; cursor: nwse-resize; }
  .poignee.s { left: calc(50% - 5px); bottom: -5px; cursor: ns-resize; }
  .poignee.sw { left: -5px; bottom: -5px; cursor: nesw-resize; }
  .poignee.w { left: -5px; top: calc(50% - 5px); cursor: ew-resize; }
</style>
