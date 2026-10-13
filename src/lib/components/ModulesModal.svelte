<script lang="ts">
  // Modale « Modules » — configuration des modules HUD de la scène
  // (horloge / musique en cours / objectifs du stream). Master-detail
  // (liste 3 modules à gauche, options à droite) comme les onglets de la
  // modale Interactions. La position/taille se règle AUSSI au drag/resize
  // directement sur le canvas (ModuleHud.svelte) — les champs X/Y/L/H de
  // ce panneau sont le complément précis.
  import {
    MODULE_IDS,
    GOAL_METRIQUES,
    goalSuggestion,
    tauri,
    type ModuleId,
    type ModuleHud,
    type GoalPlateforme,
    type GoalMetrique,
    type LigneObjectif,
    type TwitchGoal,
  } from "../tauri";
  import {
    moduleDefaut,
    setModule,
    setModuleActif,
    ajouterLigneObjectif,
    setLigneObjectif,
    retirerLigneObjectif,
    deplacerLigneObjectif,
    sceneStore,
  } from "../stores/scene";
  import { modulesModalOpen } from "../stores/ui";
  import { valeursGoals, goalsTwitch } from "../stores/objectifs";
  import { connexions, connecterTwitch } from "../stores/chat";
  import { POLICES_TITRE } from "../fonts";
  import { t, localeTag } from "../stores/langue";
  import Modal from "./Modal.svelte";

  const GOAL_PLATEFORMES: { id: GoalPlateforme; label: string }[] = [
    { id: "twitch", label: "Twitch" },
    { id: "youtube", label: "YouTube" },
    { id: "kick", label: "Kick" },
    { id: "tiktok", label: "TikTok" },
  ];
  const ICONES: Record<ModuleId, string> = {
    horloge: "🕒",
    musique: "♫",
    objectifs: "🎯",
    equaliseur: "🎚",
  };

  // Module sélectionné dans le master-detail (l'ouverture porte déjà
  // l'id — clic depuis le canvas, ou "horloge" via le lanceur Toolbar).
  let sel = $derived<ModuleId>($modulesModalOpen ?? "horloge");
  let m = $derived($sceneStore.modules?.[sel]);
  let cfg = $derived(m ?? moduleDefaut(sel, $sceneStore.canvasW ?? 1920, $sceneStore.canvasH ?? 1080));
  let lignes = $derived(cfg.lignes ?? []);

  function onFermer() {
    modulesModalOpen.set(null);
  }
  const patch = (p: Partial<ModuleHud>) => void setModule(sel, p);
  const num = (e: Event) => Math.max(0, Math.round(+(e.target as HTMLInputElement).value || 0));

  // ===== Objectifs : Creator Goals Twitch (liaison de ligne) =====
  // Même mécanique que la CarteEdition goal : liste relue (GET /goals,
  // zéro persistance), 403 → need_reauth → bouton reconnexion.
  let twitchConnecte = $derived($connexions.twitch);
  let goalsTwitchListe = $state<TwitchGoal[] | null>(null);
  let goalsErreur = $state<string | null>(null);
  let goalsChargement = $state(false);
  let lienLigneId = $state<string | null>(null); // ligne en cours de liaison

  async function chargerGoalsTwitch(): Promise<void> {
    goalsChargement = true;
    goalsErreur = null;
    try {
      goalsTwitchListe = await tauri.twitchGoalsLister();
    } catch (e) {
      goalsTwitchListe = null;
      goalsErreur = typeof e === "string" ? e : String(e);
    } finally {
      goalsChargement = false;
    }
  }

  // Auto-fetch quand le panneau Objectifs est affiché + Twitch connecté.
  $effect(() => {
    if (sel === "objectifs" && twitchConnecte) void chargerGoalsTwitch();
    else {
      goalsTwitchListe = null;
      goalsErreur = null;
    }
  });

  function onLier(ligneId: string, g: TwitchGoal): void {
    void setLigneObjectif(ligneId, { plateforme: "twitch", metrique: undefined, cible: undefined, goalId: g.id });
    lienLigneId = null;
  }
  function onDelier(ligneId: string): void {
    void setLigneObjectif(ligneId, { goalId: undefined, metrique: "followers" });
  }
  function onLignePlateforme(l: LigneObjectif, p: GoalPlateforme): void {
    const patch: Partial<LigneObjectif> = { plateforme: p };
    if (l.metrique && !GOAL_METRIQUES[p].includes(l.metrique)) patch.metrique = GOAL_METRIQUES[p][0];
    void setLigneObjectif(l.id, patch);
  }
  function ligneActuel(l: LigneObjectif): number | undefined {
    return $valeursGoals[l.plateforme ?? "twitch"]?.[l.metrique ?? "followers"];
  }
</script>

<Modal title={$t("modules.titre")} onClose={onFermer} maxWidth="54rem" crochet="var(--crochet-bleu)">
  <div class="mod-layout">
    <!-- Colonne gauche : les 4 modules (icône + nom + badge ON/OFF). -->
    <div class="mod-liste">
      {#each MODULE_IDS as id (id)}
        {@const mm = $sceneStore.modules?.[id]}
        <div class="mod-select" class:selected={sel === id}>
          <button
            class="mod-sel-btn"
            onclick={() => modulesModalOpen.set(id)}
          >
            <span class="mod-icone">{ICONES[id]}</span>
            <span class="mod-nom">{$t("modules.nom." + id)}</span>
          </button>
          <button
            class="mod-badge"
            class:on={mm?.actif}
            title={$t("modules.afficher")}
            onclick={() => void setModuleActif(id, !(mm?.actif ?? false))}
          >{mm?.actif ? "ON" : "OFF"}</button>
        </div>
      {/each}
    </div>

    <!-- Panneau droit : config du module sélectionné. -->
    <div class="mod-detail">
      <div class="modal-bloc">
        <div class="bloc-titre">{$t("modules.nom." + sel)}</div>
        <label class="toggle">
          <input type="checkbox" checked={m?.actif ?? false} onchange={(e) => patch({ actif: e.currentTarget.checked })} />
          <span>{$t("modules.afficher")}</span>
        </label>
        <p class="hint">{$t("modules.hint." + sel)}</p>
      </div>

      <!-- Position/taille — complément précis du drag/resize canvas. -->
      <div class="modal-bloc">
        <div class="bloc-titre">{$t("modules.position")}</div>
        <div class="pos-grid">
          <label class="pos-field"><span class="field-label">X</span>
            <input type="number" value={Math.round(cfg.x)} onchange={(e) => patch({ x: num(e) })} /></label>
          <label class="pos-field"><span class="field-label">Y</span>
            <input type="number" value={Math.round(cfg.y)} onchange={(e) => patch({ y: num(e) })} /></label>
          <label class="pos-field"><span class="field-label">{$t("commun.largeur")}</span>
            <input type="number" min="80" value={Math.round(cfg.largeur)} onchange={(e) => patch({ largeur: Math.max(80, num(e)) })} /></label>
          <label class="pos-field"><span class="field-label">{$t("commun.hauteur")}</span>
            <input type="number" min="36" value={Math.round(cfg.hauteur)} onchange={(e) => patch({ hauteur: Math.max(36, num(e)) })} /></label>
        </div>
      </div>

      <!-- Style : police / taille / fond / cadre. Masqué pour l'équaliseur :
           overlay diffusion sans texte ni cadre, translucide par nature —
           ces réglages n'y ont aucun effet. -->
      {#if sel !== "equaliseur"}
      <div class="modal-bloc">
        <div class="bloc-titre">{$t("modules.style")}</div>
        <div class="pos-grid">
          <label class="pos-field"><span class="field-label">{$t("commun.police")}</span>
            <select class="police-select" value={cfg.police ?? ""}
              onchange={(e) => patch({ police: (e.target as HTMLSelectElement).value || undefined })}>
              <option value="">{$t("modules.policeDefaut")}</option>
              {#each POLICES_TITRE as p (p.id)}
                <option value={p.id} style="font-family: '{p.id}', sans-serif;">{p.nom}</option>
              {/each}
            </select></label>
          <label class="pos-field"><span class="field-label">{$t("modules.taille")}</span>
            <input type="number" min="0" max="200" placeholder="auto"
              value={cfg.taille ?? ""}
              onchange={(e) => { const v = +(e.target as HTMLInputElement).value; patch({ taille: v > 0 ? Math.round(v) : undefined }); }} /></label>
          <label class="pos-field"><span class="field-label">{$t("modules.opacite")}</span>
            <input class="modal-range" type="range" min="0" max="1" step="0.05" value={cfg.opacite ?? 0.45}
              oninput={(e) => patch({ opacite: +(e.target as HTMLInputElement).value })} /></label>
          <label class="toggle pos-field"><input type="checkbox" checked={cfg.cadre === true}
              onchange={(e) => patch({ cadre: e.currentTarget.checked })} />
            <span>{$t("modules.cadre")}</span></label>
        </div>
      </div>
      {/if}

      <!-- Options spécifiques au module. -->
      <div class="modal-bloc">
        <div class="bloc-titre">{$t("modules.options")}</div>
        {#if sel === "horloge"}
          <label class="toggle"><input type="checkbox" checked={cfg.afficherDate !== false}
              onchange={(e) => patch({ afficherDate: e.currentTarget.checked })} />
            <span>{$t("modules.horloge.date")}</span></label>
          <label class="toggle"><input type="checkbox" checked={cfg.secondes === true}
              onchange={(e) => patch({ secondes: e.currentTarget.checked })} />
            <span>{$t("modules.horloge.secondes")}</span></label>
          <label class="toggle"><input type="checkbox" checked={cfg.format12h === true}
              onchange={(e) => patch({ format12h: e.currentTarget.checked })} />
            <span>{$t("modules.horloge.h12")}</span></label>
        {:else if sel === "musique"}
          <label class="toggle"><input type="checkbox" checked={cfg.afficherTemps !== false}
              onchange={(e) => patch({ afficherTemps: e.currentTarget.checked })} />
            <span>{$t("modules.musique.temps")}</span></label>
        {:else if sel === "equaliseur"}
          <label class="toggle"><input type="checkbox" checked={cfg.eqMicro !== false}
              onchange={(e) => patch({ eqMicro: e.currentTarget.checked })} />
            <span>{$t("modules.equaliseur.micro")}</span></label>
          <label class="toggle"><input type="checkbox" checked={cfg.eqBulles !== false}
              onchange={(e) => patch({ eqBulles: e.currentTarget.checked })} />
            <span>{$t("modules.equaliseur.bulles")}</span></label>
          <p class="hint">{$t("modules.equaliseur.hintMicro")}</p>
        {:else}
          <label class="pos-field"><span class="field-label">{$t("modules.objectifs.titre")}</span>
            <input type="text" value={cfg.titre ?? ""} placeholder={$t("modules.objectifs.titrePh")}
              onchange={(e) => patch({ titre: (e.target as HTMLInputElement).value || undefined })} /></label>

          <div class="lignes">
            {#each lignes as l (l.id)}
              {@const gl = l.goalId ? $goalsTwitch[l.goalId] : undefined}
              {@const act = ligneActuel(l)}
              {@const sug = act === undefined ? undefined : goalSuggestion(l.plateforme ?? "twitch", l.metrique ?? "followers", act)}
              <div class="ligne">
                <div class="ligne-haut">
                  <button class="ligne-btn" title={$t("modules.objectifs.monter")} onclick={() => void deplacerLigneObjectif(l.id, -1)}>↑</button>
                  <button class="ligne-btn" title={$t("modules.objectifs.descendre")} onclick={() => void deplacerLigneObjectif(l.id, 1)}>↓</button>
                  <input class="ligne-label" type="text" value={l.label ?? ""} placeholder={$t("modules.objectifs.labelPh")}
                    onchange={(e) => void setLigneObjectif(l.id, { label: (e.target as HTMLInputElement).value || undefined })} />
                  <button class="ligne-btn suppr" title={$t("commun.supprimer")} onclick={() => void retirerLigneObjectif(l.id)}>✕</button>
                </div>
                {#if l.goalId}
                  <div class="ligne-lie">
                    <span class="ligne-lie-nom">
                      {#if goalsTwitchListe !== null && !goalsTwitchListe.some((g) => g.id === l.goalId)}
                        {$t("carteEdition.goalDisparu")}
                      {:else}
                        {gl?.description || $t("goal.ttype." + (gl?.goal_type ?? ""))}
                      {/if}
                    </span>
                    {#if gl}<span class="ligne-lie-val">{gl.actuel.toLocaleString($localeTag)} / {gl.cible.toLocaleString($localeTag)}</span>{/if}
                    <button class="action" onclick={() => onDelier(l.id)}>{$t("carteEdition.goalDelier")}</button>
                  </div>
                {:else}
                  <div class="ligne-champs">
                    <select value={l.plateforme ?? "twitch"}
                      onchange={(e) => onLignePlateforme(l, (e.target as HTMLSelectElement).value as GoalPlateforme)}>
                      {#each GOAL_PLATEFORMES as p (p.id)}<option value={p.id}>{p.label}</option>{/each}
                    </select>
                    <select value={l.metrique ?? "followers"}
                      onchange={(e) => void setLigneObjectif(l.id, { metrique: (e.target as HTMLSelectElement).value as GoalMetrique })}>
                      {#each GOAL_METRIQUES[l.plateforme ?? "twitch"] as mm (mm)}
                        <option value={mm}>{$t("goal.metrique." + mm)}</option>
                      {/each}
                    </select>
                    <input class="ligne-cible" type="number" min="1" step="1" value={l.cible ?? ""}
                      placeholder={$t("carteEdition.cible")}
                      onchange={(e) => { const v = Math.max(1, Math.round(+(e.target as HTMLInputElement).value || 0)); void setLigneObjectif(l.id, { cible: v }); }} />
                    <button class="action" disabled={sug === undefined}
                      onclick={() => sug !== undefined && void setLigneObjectif(l.id, { cible: sug })}>
                      {$t("carteEdition.suggerer", { v: sug === undefined ? "N/A" : sug.toLocaleString($localeTag) })}
                    </button>
                    {#if (l.plateforme ?? "twitch") === "twitch"}
                      <button class="action" class:actif={lienLigneId === l.id}
                        onclick={() => (lienLigneId = lienLigneId === l.id ? null : l.id)}>
                        {$t("modules.objectifs.lier")}
                      </button>
                    {/if}
                  </div>
                {/if}
              </div>
            {/each}
          </div>
          <button class="action" onclick={() => void ajouterLigneObjectif()}>{$t("modules.objectifs.ajouter")}</button>

          {#if lienLigneId !== null}
            <!-- Liste des Creator Goals Twitch pour la ligne en liaison —
                 même mécanique que la CarteEdition goal. -->
            <div class="fit-group">
              <span class="field-label">{$t("carteEdition.goalsTwitch")}</span>
              {#if !twitchConnecte}
                <p class="hint">{$t("carteEdition.goalsNonConnecte")}</p>
              {:else if goalsChargement && goalsTwitchListe === null}
                <p class="hint">{$t("carteEdition.goalsChargement")}</p>
              {:else if goalsErreur === "need_reauth"}
                <p class="hint">{$t("carteEdition.goalsReauth")}</p>
                <button class="action" onclick={() => void connecterTwitch()}>{$t("carteEdition.reconnecterTwitch")}</button>
              {:else if goalsErreur}
                <p class="hint">{goalsErreur}</p>
                <button class="action" onclick={() => void chargerGoalsTwitch()}>{$t("carteEdition.goalsRecharger")}</button>
              {:else if goalsTwitchListe !== null && goalsTwitchListe.length === 0}
                <p class="hint">{$t("carteEdition.goalsVide")}</p>
              {:else if goalsTwitchListe !== null}
                <div class="goal-liste">
                  {#each goalsTwitchListe as g (g.id)}
                    {@const gl2 = $goalsTwitch[g.id] ?? g}
                    <button class="goal-item" onclick={() => lienLigneId && onLier(lienLigneId, gl2)}>
                      <span class="goal-item-haut">
                        <span class="goal-item-nom">{gl2.description || $t("goal.ttype." + gl2.goal_type)}</span>
                        {#if gl2.termine}<span class="goal-item-badge">{$t("carteEdition.goalTermine")}</span>{/if}
                      </span>
                      <span class="goal-item-chiffres">{gl2.actuel.toLocaleString($localeTag)} / {gl2.cible.toLocaleString($localeTag)}</span>
                      <span class="goal-item-bar"><span class="goal-item-fill" style:width={(gl2.cible > 0 ? Math.min(100, Math.round((gl2.actuel / gl2.cible) * 100)) : 0) + "%"}></span></span>
                    </button>
                  {/each}
                </div>
              {/if}
            </div>
          {/if}
        {/if}
      </div>
    </div>
  </div>
</Modal>

<style>
  .mod-layout { display: flex; flex: 1; min-height: 0; }
  .mod-liste {
    width: 12.5rem;
    flex-shrink: 0;
    border-right: 1px solid var(--bordure-ui);
    padding: 0.6rem;
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
    overflow-y: auto;
  }
  .mod-select {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.3rem;
    border-radius: var(--rayon);
    background: var(--btn-surface);
    box-shadow: var(--btn-inset);
    border: 1px solid transparent;
    color: var(--texte);
    transition: border-color 0.15s ease, background 0.15s ease;
  }
  .mod-select:hover { background: var(--btn-surface-hover); }
  .mod-select.selected { border-color: var(--accent-ui); }
  .mod-sel-btn {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.2rem 0.3rem;
    background: transparent;
    border: none;
    color: var(--texte);
    cursor: pointer;
    text-align: left;
  }
  .mod-icone { font-size: 0.95rem; width: 1.2rem; text-align: center; }
  .mod-nom { flex: 1; font-size: 0.82rem; font-weight: 600; }
  .mod-badge {
    flex: none;
    font-family: inherit;
    font-size: 0.6rem;
    font-weight: 700;
    letter-spacing: 0.06em;
    padding: 0.18rem 0.5rem;
    border: 1px solid transparent;
    border-radius: 999px;
    background: rgba(255, 255, 255, 0.08);
    color: var(--gris, #9a9aa5);
    cursor: pointer;
  }
  .mod-badge:hover { border-color: var(--accent-ui); }
  /* ON = vert sémantique (même convention que .alerte-toggle.on /
     .dot.on : --message-ok-color plein + texte blanc). */
  .mod-badge.on {
    background: var(--message-ok-color);
    border-color: var(--message-ok-color);
    color: #fff;
  }
  .mod-badge.on:hover {
    background: var(--message-ok-color);
    border-color: var(--message-ok-color);
    color: #fff;
  }
  .mod-detail { flex: 1; min-width: 0; overflow-y: auto; padding: 0.8rem 1rem; display: flex; flex-direction: column; gap: 0.7rem; }

  .bloc-titre { font-size: 0.75rem; font-weight: 700; letter-spacing: 0.06em; text-transform: uppercase; opacity: 0.8; }
  .toggle { display: flex; align-items: center; gap: 0.45rem; font-size: 0.82rem; cursor: pointer; }
  .fit-group { display: flex; flex-direction: column; gap: 0.4rem; }

  .pos-grid { display: grid; grid-template-columns: repeat(4, 1fr); gap: 0.5rem; }
  .pos-field { display: flex; flex-direction: column; gap: 0.25rem; min-width: 0; }
  .pos-field input[type="number"], .pos-field input[type="text"], .pos-field select { width: 100%; }
  .police-select { width: 100%; }
  /* Inputs texte/nombre : recette commune des autres modales (fond
     contrôle + bordure neutre, accent au focus) — app.css ne couvre
     que les selects, sans ce bloc les inputs rendent en natif clair. */
  .mod-detail input[type="text"],
  .mod-detail input[type="number"] {
    background: var(--fond-controle);
    color: var(--texte);
    border: 1px solid var(--bordure-ui);
    padding: 0.3rem 0.45rem;
    font: inherit;
    font-size: 0.85rem;
  }
  .mod-detail input[type="text"]:focus,
  .mod-detail input[type="number"]:focus {
    outline: none;
    border-color: var(--accent-ui);
  }
  /* Bouton « Lier » en mode liaison : .action.actif (recette globale
     app.css — bordure + texte accent, même convention que .chip.actif). */

  .lignes { display: flex; flex-direction: column; gap: 0.45rem; }
  .ligne { border: 1px solid var(--bordure-ui); border-radius: var(--rayon); padding: 0.45rem 0.55rem; display: flex; flex-direction: column; gap: 0.4rem; }
  .ligne-haut { display: flex; align-items: center; gap: 0.3rem; }
  .ligne-label { flex: 1; min-width: 0; }
  .ligne-btn {
    flex: none;
    width: 1.4rem;
    height: 1.4rem;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--btn-surface);
    box-shadow: var(--btn-inset);
    border: 1px solid var(--bordure-ui);
    color: var(--texte);
    cursor: pointer;
    border-radius: var(--rayon);
    font-size: 0.7rem;
    line-height: 1;
  }
  .ligne-btn:hover { background: var(--btn-surface-hover); }
  .ligne-btn.suppr:hover { color: var(--message-error-color); border-color: var(--message-error-color); }
  .ligne-champs { display: flex; flex-wrap: wrap; gap: 0.4rem; align-items: center; }
  .ligne-cible { width: 5.5rem; }
  .ligne-lie { display: flex; align-items: center; gap: 0.5rem; flex-wrap: wrap; }
  .ligne-lie-nom { flex: 1; min-width: 0; font-size: 0.78rem; font-weight: 600; }
  .ligne-lie-val { font-size: 0.72rem; opacity: 0.75; font-variant-numeric: tabular-nums; }

  .goal-liste { display: flex; flex-direction: column; gap: 0.3rem; }
  .goal-item {
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
    padding: 0.4rem 0.55rem;
    border-radius: var(--rayon);
    background: var(--btn-surface);
    box-shadow: var(--btn-inset);
    border: 1px solid var(--bordure-ui);
    color: var(--texte);
    cursor: pointer;
    text-align: left;
  }
  .goal-item:hover { background: var(--btn-surface-hover); border-color: var(--accent-ui); }
  .goal-item-haut { display: flex; align-items: center; gap: 0.4rem; }
  .goal-item-nom { flex: 1; min-width: 0; font-size: 0.78rem; font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .goal-item-badge { font-size: 0.6rem; font-weight: 700; padding: 0.08rem 0.35rem; border-radius: 999px; background: color-mix(in srgb, var(--accent-ui) 25%, transparent); }
  .goal-item-chiffres { font-size: 0.7rem; opacity: 0.75; font-variant-numeric: tabular-nums; }
  .goal-item-bar { height: 0.25rem; border-radius: 999px; background: rgba(255, 255, 255, 0.12); overflow: hidden; }
  .goal-item-fill { display: block; height: 100%; background: var(--accent-ui); }
</style>
