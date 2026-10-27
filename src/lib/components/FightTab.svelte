<script lang="ts">
  // Onglet « Emoji Arena » de la modale « Interactions Live » — combats
  // d'emojis 1v1 tour par tour pilotés par le chat (!fight, !a/!b/!c),
  // viewer vs viewer (multi-plateforme possible) ou viewer vs BOT.
  //
  // L'état (config + partie + cooldowns) vit côté Rust et arrive par
  // l'event "fight:update" — le store fightEtat est la seule source de
  // vérité affichée. Les réglages sont édités dans une copie locale (cfg)
  // et sauvegardés en un clic (fight_set_config — noms normalisés et
  // collisions refusées côté Rust). Les boutons Tester/Stop agissent sur
  // la partie en cours (pipeline réel, pas de simulation locale).
  import { onMount } from "svelte";
  import { get } from "svelte/store";
  import { t } from "../stores/langue";
  import {
    fightEtat,
    initFight,
    sauverConfigFight,
    testerFight,
    stopFight,
    previsualiserAideFight,
  } from "../stores/fight";
  import type { FightConfig } from "../tauri";

  /// Combattant tel que sérialisé dans la frame fight-state (fight.rs).
  interface FightJoueur {
    pseudo?: string;
    plateforme?: string;
    emoji?: string;
    pv?: number;
    pv_max?: number;
    bot?: boolean;
    special_utilise?: boolean;
  }

  let etat = $derived($fightEtat);
  let partie = $derived(etat?.partie ?? null);
  let phase = $derived((partie?.phase as string | undefined) ?? "idle");
  let ja = $derived((partie?.ja ?? null) as FightJoueur | null);
  let jb = $derived((partie?.jb ?? null) as FightJoueur | null);
  let defiant = $derived((partie?.defiant ?? null) as FightJoueur | null);
  let gagnant = $derived((partie?.gagnant as string | null | undefined) ?? null);
  let raison = $derived((partie?.raison as string | undefined) ?? "ko");
  let cooldowns = $derived(etat?.cooldowns ?? []);

  /// Copie locale éditable de la config (sauvegarde explicite — un clic).
  /// NE PAS resync sur chaque fight:update (écraserait l'édition en cours).
  let cfg = $state<FightConfig | null>(null);
  /// Confirmation inline pour l'action destructive (forcer l'arrêt).
  let confirm = $state<{ texte: string; action: () => Promise<void> } | null>(
    null
  );
  let msg = $state<{ texte: string; ok: boolean } | null>(null);

  onMount(async () => {
    await initFight();
    if (!cfg) cfg = get(fightEtat)?.config ?? null;
  });

  async function onConfirmer(): Promise<void> {
    const a = confirm?.action;
    confirm = null;
    if (a) await a();
  }

  async function onEnregistrer(): Promise<void> {
    if (!cfg) return;
    const ok = await sauverConfigFight(cfg);
    msg = {
      texte: get(t)(ok ? "interactions.fight.sauveOk" : "interactions.fight.sauveKo"),
      ok,
    };
  }

  async function onToggleActif(): Promise<void> {
    if (!cfg) return;
    cfg = { ...cfg, actif: !cfg.actif };
    await sauverConfigFight(cfg);
  }

  async function onTester(): Promise<void> {
    const ok = await testerFight();
    msg = {
      texte: get(t)(ok ? "interactions.fight.testOk" : "interactions.fight.testKo"),
      ok,
    };
  }

  function onStop(): void {
    confirm = {
      texte: get(t)("interactions.fight.confirmStop"),
      action: async () => {
        await stopFight();
      },
    };
  }

  /// Message d'accueil à coller dans le chat (épinglé / panneau / commande).
  /// Construit avec les noms de commandes COURANTS de la copie locale.
  function messageAccueil(): string {
    const c = cfg;
    if (!c) return "";
    const f = c.commande_fight;
    const minutes = Math.max(1, Math.round(c.duree_match_s / 60));
    return [
      "⚔ ARÈNE D'EMOJIS",
      `!${f} · !${f} @pseudo · !${f} bot · !${c.commande_accept} 🐲`,
      `!${c.commande_a} rapide 8-12 (90%) · !${c.commande_b} lourd 18-26 (55%) · !${c.commande_c} spécial vol de vie (1×)`,
      `${c.duree_tour_s} s/tour · ${minutes} min max · !${c.commande_ff} abandon`,
      `!${f} aide pour les règles · !${f} liste pour les emojis`,
    ].join("\n");
  }

  async function onCopier(): Promise<void> {
    try {
      await navigator.clipboard.writeText(messageAccueil());
      msg = { texte: get(t)("interactions.fight.copieOk"), ok: true };
    } catch {
      msg = { texte: get(t)("interactions.fight.copieKo"), ok: false };
    }
  }

  async function onPrevisualiser(): Promise<void> {
    await previsualiserAideFight();
  }

  function nomPhase(p: string): string {
    switch (p) {
      case "lobby":
        return get(t)("interactions.fight.phaseLobby");
      case "combat":
        return get(t)("interactions.fight.phaseCombat");
      case "fin":
        return get(t)("interactions.fight.phaseFin");
      default:
        return get(t)("interactions.fight.phaseIdle");
    }
  }

  function nomJoueur(j: FightJoueur | null): string {
    if (!j) return "?";
    return j.bot ? get(t)("interactions.fight.bot") : (j.pseudo ?? "?");
  }
</script>

<div class="fight-layout">
  <!-- Encart explicatif : mécanique tour par tour + règle des coups +
       multi-plateforme (viewer Twitch vs viewer Kick…). -->
  <div class="modal-bloc bloc-info">
    <div class="modal-bloc-titre">{$t("interactions.fight.titreInfo")}</div>
    <p class="hint">{$t("interactions.fight.intro")}</p>
    <p class="hint">{$t("interactions.fight.introCoups")}</p>
    <p class="hint">{$t("interactions.fight.introMulti")}</p>
  </div>

  {#if !etat}
    <p class="hint-centre">…</p>
  {:else}
    <!-- Partie en cours + actions live -->
    <div class="modal-bloc">
      <div class="modal-bloc-titre">
        {$t("interactions.fight.enCours")}
        {#if etat.config.actif}
          <span class="etat on">ON</span>
        {:else}
          <span class="etat off">OFF</span>
        {/if}
        <span class="spacer"></span>
        <button class="action small" onclick={() => void onTester()}>
          ⚔ {$t("interactions.fight.tester")}
        </button>
        {#if phase !== "idle"}
          <button
            class="action small danger"
            title={$t("interactions.fight.stop")}
            onclick={onStop}>🛑 {$t("interactions.fight.stop")}</button
          >
        {/if}
      </div>

      {#if phase === "idle"}
        <p class="hint">{$t("interactions.fight.aucunePartie")}</p>
      {:else if phase === "lobby"}
        <div class="partie-ligne">
          <span class="phase-tag">{nomPhase(phase)}</span>
          <span class="emoji-big">{defiant?.emoji ?? "❓"}</span>
          <span>{nomJoueur(defiant)}</span>
          {#if partie?.cible}
            <span class="hint">
              → @{partie.cible} —
              !{etat.config.commande_accept} / !{etat.config.commande_refuse}
            </span>
          {:else}
            <span class="hint">— !{etat.config.commande_accept}</span>
          {/if}
        </div>
      {:else}
        <!-- combat / fin : les deux combattants + PV -->
        <div class="partie-ligne">
          <span class="phase-tag">{nomPhase(phase)}</span>
          <span class="emoji-big">{ja?.emoji ?? "❓"}</span>
          <span class="nom-joueur">{nomJoueur(ja)}</span>
          <span class="pv">{ja?.pv ?? 0}/{ja?.pv_max ?? 0} PV</span>
          {#if ja?.special_utilise}
            <span class="chip-off">{$t("interactions.fight.specialUtilise")}</span>
          {/if}
          <span class="vs">{$t("interactions.fight.vs")}</span>
          <span class="pv">{jb?.pv ?? 0}/{jb?.pv_max ?? 0} PV</span>
          {#if jb?.special_utilise}
            <span class="chip-off">{$t("interactions.fight.specialUtilise")}</span>
          {/if}
          <span class="nom-joueur">{nomJoueur(jb)}</span>
          <span class="emoji-big">{jb?.emoji ?? "❓"}</span>
        </div>
        {#if phase === "fin"}
          <p class="hint">
            {#if raison === "afk"}
              {$t("interactions.fight.matchAfk")}
            {:else if raison === "stop"}
              {$t("interactions.fight.matchStop")}
            {:else if gagnant === "a"}
              {$t("interactions.fight.gagnant", { pseudo: nomJoueur(ja) })}
            {:else if gagnant === "b"}
              {$t("interactions.fight.gagnant", { pseudo: nomJoueur(jb) })}
            {:else}
              {$t("interactions.fight.egalite")}
            {/if}
          </p>
        {/if}
      {/if}

      {#if cooldowns.length > 0}
        <p class="hint cooldowns">
          {$t("interactions.fight.cooldownsActifs", { n: cooldowns.length })}
          {cooldowns.map((c) => `${c.joueur} (${c.restant_s}s)`).join(" · ")}
        </p>
      {/if}
    </div>

    <!-- Message d'accueil : à coller dans le chat (épinglé / panneau) -->
    {#if cfg}
      <div class="modal-bloc">
        <div class="modal-bloc-titre">{$t("interactions.fight.accueilTitre")}</div>
        <pre class="accueil">{messageAccueil()}</pre>
        <div class="row-actions">
          <button class="action small" onclick={() => void onCopier()}>
            {$t("interactions.fight.copierAccueil")}
          </button>
          <button class="action small" onclick={() => void onPrevisualiser()}>
            {$t("interactions.fight.previsualiserAide")}
          </button>
        </div>
      </div>
    {/if}

    <!-- Réglages -->
    {#if cfg}
      <div class="modal-bloc">
        <div class="modal-bloc-titre">{$t("interactions.fight.reglages")}</div>
        <label class="row">
          <input
            type="checkbox"
            checked={cfg.actif}
            onchange={() => void onToggleActif()}
          />
          {$t("interactions.fight.actif")}
        </label>
        <div class="grid-cmds">
          <label class="field">
            <span class="field-label">{$t("interactions.fight.commandeFight")}</span>
            <span class="cmd">!<input type="text" bind:value={cfg.commande_fight} /></span>
          </label>
          <label class="field">
            <span class="field-label">{$t("interactions.fight.commandeAccept")}</span>
            <span class="cmd">!<input type="text" bind:value={cfg.commande_accept} /></span>
          </label>
          <label class="field">
            <span class="field-label">{$t("interactions.fight.commandeRefuse")}</span>
            <span class="cmd">!<input type="text" bind:value={cfg.commande_refuse} /></span>
          </label>
          <label class="field">
            <span class="field-label">{$t("interactions.fight.commandeFf")}</span>
            <span class="cmd">!<input type="text" bind:value={cfg.commande_ff} /></span>
          </label>
        </div>
        <div class="grid-cmds">
          <label class="field">
            <span class="field-label">{$t("interactions.fight.commandeA")}</span>
            <span class="cmd">!<input type="text" bind:value={cfg.commande_a} /></span>
          </label>
          <label class="field">
            <span class="field-label">{$t("interactions.fight.commandeB")}</span>
            <span class="cmd">!<input type="text" bind:value={cfg.commande_b} /></span>
          </label>
          <label class="field">
            <span class="field-label">{$t("interactions.fight.commandeC")}</span>
            <span class="cmd">!<input type="text" bind:value={cfg.commande_c} /></span>
          </label>
          <label class="field">
            <span class="field-label">{$t("interactions.fight.commandeStop")}</span>
            <span class="cmd">!<input type="text" bind:value={cfg.commande_stop} /></span>
          </label>
        </div>
        <div class="grid-cmds">
          <label class="field">
            <span class="field-label">{$t("interactions.fight.vieMax")}</span>
            <input type="number" min="10" max="1000" bind:value={cfg.vie_max} />
          </label>
          <label class="field">
            <span class="field-label">{$t("interactions.fight.dureeLobby")}</span>
            <input type="number" min="5" max="120" bind:value={cfg.duree_lobby_s} />
          </label>
          <label class="field">
            <span class="field-label">{$t("interactions.fight.dureeTour")}</span>
            <input type="number" min="5" max="60" bind:value={cfg.duree_tour_s} />
          </label>
          <label class="field">
            <span class="field-label">{$t("interactions.fight.dureeMatch")}</span>
            <input type="number" min="30" max="900" bind:value={cfg.duree_match_s} />
          </label>
          <label class="field">
            <span class="field-label">{$t("interactions.fight.cooldown")}</span>
            <input type="number" min="0" max="3600" bind:value={cfg.cooldown_combat_s} />
          </label>
        </div>
        <label class="row">
          <input type="checkbox" bind:checked={cfg.reponses_chat} />
          {$t("interactions.fight.reponsesChat")}
        </label>
        <label class="row">
          <input type="checkbox" bind:checked={cfg.annonce_tour} />
          {$t("interactions.fight.annonceTour")}
        </label>
        <label class="row">
          <input type="checkbox" bind:checked={cfg.rappel_tour} />
          {$t("interactions.fight.rappelTour")}
        </label>
        <label class="row">
          <input type="checkbox" bind:checked={cfg.animations_reduites} />
          {$t("interactions.fight.animationsReduites")}
        </label>
        <div>
          <button class="action" onclick={() => void onEnregistrer()}>
            {$t("interactions.fight.enregistrer")}
          </button>
        </div>
        <p class="hint">{$t("interactions.fight.hintModeration")}</p>
      </div>
    {/if}
  {/if}

  <!-- Message de retour + confirmation inline -->
  {#if msg}
    <p class="retour" class:ko={!msg.ok}>{msg.texte}</p>
  {/if}
  {#if confirm}
    <div class="confirm">
      <span>{confirm.texte}</span>
      <button class="action small danger" onclick={() => void onConfirmer()}>
        {$t("interactions.fight.confirmer")}
      </button>
      <button class="action small" onclick={() => (confirm = null)}>
        {$t("interactions.fight.annuler")}
      </button>
    </div>
  {/if}
</div>

<style>
  .fight-layout {
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
  }

  .bloc-info {
    border-color: color-mix(in srgb, var(--accent-ui) 40%, var(--bordure-ui));
  }
  .modal-bloc-titre {
    display: flex;
    align-items: center;
    gap: 0.4rem;
  }
  .spacer {
    flex: 1;
  }
  .etat {
    font-size: 0.65rem;
    font-weight: 700;
    padding: 0 0.35rem;
    border-radius: 0.25rem;
  }
  .etat.on {
    background: color-mix(in srgb, var(--accent-ui) 30%, transparent);
  }
  .etat.off {
    background: color-mix(in srgb, var(--message-error-color) 25%, transparent);
    color: var(--message-error-color);
  }

  .partie-ligne {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    flex-wrap: wrap;
    font-size: 0.9rem;
    padding: 0.4rem 0.2rem;
  }
  .phase-tag {
    font-size: 0.7rem;
    font-weight: 700;
    padding: 0.15rem 0.45rem;
    border-radius: 999px;
    background: color-mix(in srgb, var(--accent-ui) 25%, transparent);
    white-space: nowrap;
  }
  .emoji-big {
    font-size: 1.5rem;
    line-height: 1;
  }
  .nom-joueur {
    font-weight: 600;
  }
  .pv {
    font-size: 0.78rem;
    opacity: 0.75;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .vs {
    font-weight: 800;
    color: var(--accent-ui);
  }
  .cooldowns {
    margin-top: 0.3rem;
  }
  .accueil {
    margin: 0;
    padding: 0.55rem 0.7rem;
    border: 1px solid var(--bordure-ui);
    border-radius: var(--rayon-petit);
    background: color-mix(in srgb, var(--fond-panneau) 82%, black);
    font: 0.78rem/1.45 ui-monospace, monospace;
    white-space: pre-wrap;
    user-select: text;
  }
  .row-actions {
    display: flex;
    gap: 0.4rem;
    flex-wrap: wrap;
  }
  .chip-off {
    font-size: 0.65rem;
    font-weight: 700;
    padding: 0 0.35rem;
    border-radius: 999px;
    background: color-mix(in srgb, var(--message-error-color) 22%, transparent);
    color: var(--message-error-color);
    white-space: nowrap;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    font-size: 0.85rem;
  }
  .grid-cmds {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(8.75rem, 1fr));
    gap: 0.5rem;
  }
  .cmd {
    display: flex;
    align-items: center;
    gap: 0.15rem;
    font-size: 0.85rem;
  }
  .hint-centre {
    font-size: 0.9rem;
    opacity: 0.6;
    text-align: center;
    margin: 2rem 0;
  }
  .retour {
    font-size: 0.8rem;
    margin: 0;
    color: var(--accent-ui);
  }
  .retour.ko {
    color: var(--message-error-color);
  }
  .confirm {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    font-size: 0.8rem;
    padding: 0.4rem 0.6rem;
    border: 1px solid
      color-mix(in srgb, var(--message-error-color) 40%, var(--bordure-ui));
    border-radius: var(--rayon-petit);
    position: sticky;
    bottom: 0;
    background: var(--fond-panneau);
  }
</style>
