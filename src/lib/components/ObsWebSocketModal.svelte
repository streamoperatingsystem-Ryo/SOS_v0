<script lang="ts">
  // Modale « Connexion OBS » — la fenêtre WebSocket pour les curieux.
  //
  // Deux fonctions :
  //   1. EXPLIQUER ce qu'est le WebSocket d'OBS (obs-websocket v5, OBS ≥ 28)
  //      et INVENTORIER tout ce qui y transite — la pastille de chaque ligne
  //      reflète l'état RÉEL (stores), ce n'est pas un interrupteur.
  //   2. RÉGLER les rares comportements optionnels (section « Réglages ») :
  //      connexion auto au boot, fermeture des projecteurs, mute audio
  //      bureau de l'écran d'attente. Tout le reste est structurel — une
  //      fonctionnalité se pilote depuis sa propre section, pas d'ici.
  //
  // ⚠️ Pas de connexion persistante : chaque action = un one-shot
  // (connect → requête → close) — c'est le point pédagogique du bloc intro.
  import { get } from "svelte/store";
  import { onMount } from "svelte";
  import Modal from "./Modal.svelte";
  import { t } from "../stores/langue";
  import {
    obsStatus,
    obsError,
    obsHost,
    obsPort,
    obsStreaming,
    obsRecording,
    obsAutoConnect,
    obsFermerProjecteurs,
  } from "../stores/obs";
  import { sceneStore } from "../stores/scene";
  import { jcActif } from "../stores/justChatting";
  import {
    ecranAttenteStore,
    ecranAttenteSetConfig,
    chargerEcranAttente,
  } from "../stores/ecranAttente";
  import { obsWebSocketModalOpen } from "../stores/ui";

  function onFermer() {
    obsWebSocketModalOpen.set(false);
  }

  // Config écran d'attente relue à l'ouverture (le toggle muter_bureau en
  // dépend — elle peut ne pas être chargée si l'écran n'a jamais été ouvert).
  onMount(() => {
    void chargerEcranAttente();
  });

  // ===== État réel de chaque fonctionnalité (pastilles de l'inventaire) =====
  let status = $derived($obsStatus);
  let connecte = $derived(status === "connected");
  let streaming = $derived($obsStreaming);
  let recording = $derived($obsRecording);
  let nbTrous = $derived($sceneStore.widgets.filter((w) => w.obsSource).length);
  let hasCamera = $derived($sceneStore.widgets.some((w) => w.type === "camera"));
  let fondCapture = $derived(!!$sceneStore.bgObsSource);
  let eaActif = $derived($ecranAttenteStore?.actif ?? false);
  let eaMuter = $derived($ecranAttenteStore?.config.muter_bureau ?? true);
  let jcOn = $derived($jcActif);

  // Ligne d'inventaire : id (racine des clés i18n obsWs.feat.<id>.*) + état.
  // etat: "on" (pastille verte + badge actif), "off" (gris + inactif),
  // "demande" (gris + « à la demande »), "n" (vert + badge « {n} liée(s) »).
  type EtatFeat = "on" | "off" | "demande" | "n";
  type Feat = { id: string; etat: EtatFeat; n?: number };

  let featsDiffusion = $derived<Feat[]>([
    { id: "diffusion", etat: connecte ? "on" : "off" },
    { id: "captures", etat: nbTrous > 0 ? "n" : "off", n: nbTrous },
    { id: "camera", etat: hasCamera ? "on" : "off" },
    { id: "fond", etat: fondCapture ? "on" : "off" },
  ]);
  let featsDirect = $derived<Feat[]>([
    { id: "direct", etat: streaming || recording ? "on" : "off" },
    { id: "projecteur", etat: "demande" },
  ]);
  let featsModes = $derived<Feat[]>([
    { id: "attente", etat: eaActif ? "on" : "off" },
    { id: "jc", etat: jcOn ? "on" : "off" },
  ]);

  // tr passé en argument depuis le template ({badge(f, $t)}) : garder $t
  // dans l'expression rendue conserve la réactivité au changement de langue.
  function badge(
    f: Feat,
    tr: (cle: string, vars?: Record<string, string | number>) => string,
  ): string {
    switch (f.etat) {
      case "on":
        return tr("obsWs.etat.actif");
      case "demande":
        return tr("obsWs.etat.aLaDemande");
      case "n":
        return tr("obsWs.etat.liees", { n: f.n ?? 0 });
      default:
        return tr("obsWs.etat.inactif");
    }
  }

  // ===== Réglages =====
  // autoConnect / fermerProjecteurs : stores obs.ts persistés en
  // localStorage — le set suffit.
  function onCheck(e: Event, setter: (v: boolean) => void): void {
    setter((e.target as HTMLInputElement).checked);
  }

  // muter_bureau : champ de la config écran d'attente (persistée dans
  // ecran_attente.json côté Rust) — set via la commande existante.
  async function onMuterBureau(e: Event): Promise<void> {
    const checked = (e.target as HTMLInputElement).checked;
    const etat = get(ecranAttenteStore);
    if (!etat) return;
    try {
      await ecranAttenteSetConfig({ ...etat.config, muter_bureau: checked });
    } catch (err) {
      console.warn("obsWs muter_bureau:", err);
    }
  }
</script>

<Modal
  title={$t("obsWs.titre")}
  onClose={onFermer}
  maxWidth="46rem"
  hint={$t("obsWs.hint")}
  crochet="var(--crochet-dore)"
>
  <div class="obsws-body">
    <!-- Intro : qu'est-ce qu'un WebSocket + version requise + one-shot. -->
    <div class="modal-bloc">
      <div class="bloc-titre">{$t("obsWs.blocCestQuoi")}</div>
      <p class="ws-para">{$t("obsWs.intro.p1")}</p>
      <p class="ws-para">{$t("obsWs.intro.p2")}</p>
      <p class="ws-para">{$t("obsWs.intro.p3")}</p>
    </div>

    <!-- État de la connexion (le même que l'en-tête de la section OBS). -->
    <div class="modal-bloc">
      <div class="bloc-titre">{$t("obsWs.blocEtat")}</div>
      <div class="etat-ligne">
        <span class="feat-dot" class:on={connecte}></span>
        <span class="etat-texte">
          {status === "connected"
            ? $t("obsWs.connexion.ok")
            : status === "connecting"
              ? $t("obsWs.connexion.enCours")
              : $t("obsWs.connexion.hs")}
        </span>
        <span class="etat-adr">{$obsHost}:{$obsPort}</span>
      </div>
      {#if status === "error" && $obsError}
        <p class="hint msg-warn">{$obsError}</p>
      {/if}
      <p class="hint">{$t("obsWs.version")}</p>
    </div>

    <!-- Inventaire : tout ce qui transite par le WebSocket OBS. -->
    <div class="modal-bloc">
      <div class="bloc-titre">{$t("obsWs.blocInventaire")}</div>
      <p class="hint">{$t("obsWs.inventaireHint")}</p>

      <div class="feat-groupe">{$t("obsWs.groupe.diffusion")}</div>
      {#each featsDiffusion as f (f.id)}
        <div class="feat-ligne">
          <span class="feat-dot" class:on={f.etat === "on" || f.etat === "n"}></span>
          <div class="feat-txt">
            <span class="feat-nom">{$t("obsWs.feat." + f.id + ".nom")}</span>
            <span class="feat-desc">{$t("obsWs.feat." + f.id + ".desc")}</span>
          </div>
          <span class="feat-etat" class:on={f.etat === "on" || f.etat === "n"}>{badge(f, $t)}</span>
        </div>
      {/each}

      <div class="feat-groupe">{$t("obsWs.groupe.direct")}</div>
      {#each featsDirect as f (f.id)}
        <div class="feat-ligne">
          <span class="feat-dot" class:on={f.etat === "on" || f.etat === "n"}></span>
          <div class="feat-txt">
            <span class="feat-nom">{$t("obsWs.feat." + f.id + ".nom")}</span>
            <span class="feat-desc">{$t("obsWs.feat." + f.id + ".desc")}</span>
          </div>
          <span class="feat-etat" class:on={f.etat === "on" || f.etat === "n"}>{badge(f, $t)}</span>
        </div>
      {/each}

      <div class="feat-groupe">{$t("obsWs.groupe.modes")}</div>
      {#each featsModes as f (f.id)}
        <div class="feat-ligne">
          <span class="feat-dot" class:on={f.etat === "on" || f.etat === "n"}></span>
          <div class="feat-txt">
            <span class="feat-nom">{$t("obsWs.feat." + f.id + ".nom")}</span>
            <span class="feat-desc">{$t("obsWs.feat." + f.id + ".desc")}</span>
          </div>
          <span class="feat-etat" class:on={f.etat === "on" || f.etat === "n"}>{badge(f, $t)}</span>
        </div>
      {/each}
    </div>

    <!-- Réglages : les seuls interrupteurs — comportements optionnels. -->
    <div class="modal-bloc">
      <div class="bloc-titre">{$t("obsWs.blocReglages")}</div>
      <label class="toggle">
        <input
          type="checkbox"
          checked={$obsAutoConnect}
          onchange={(e) => onCheck(e, (v) => obsAutoConnect.set(v))}
        />
        <span>{$t("obsWs.reglages.autoConnect")}</span>
      </label>
      <p class="hint">{$t("obsWs.reglages.autoConnectDesc")}</p>
      <label class="toggle">
        <input
          type="checkbox"
          checked={$obsFermerProjecteurs}
          onchange={(e) => onCheck(e, (v) => obsFermerProjecteurs.set(v))}
        />
        <span>{$t("obsWs.reglages.fermerProj")}</span>
      </label>
      <p class="hint">{$t("obsWs.reglages.fermerProjDesc")}</p>
      <label class="toggle">
        <input
          type="checkbox"
          checked={eaMuter}
          disabled={!$ecranAttenteStore}
          onchange={onMuterBureau}
        />
        <span>{$t("obsWs.reglages.muterBureau")}</span>
      </label>
      <p class="hint">{$t("obsWs.reglages.muterBureauDesc")}</p>
    </div>

    <!-- Ce qui ne passe PAS par ce canal — pour éviter les fausses idées. -->
    <div class="modal-bloc">
      <div class="bloc-titre">{$t("obsWs.blocHors")}</div>
      <ul class="ws-liste">
        <li>{$t("obsWs.hors.l1")}</li>
        <li>{$t("obsWs.hors.l2")}</li>
        <li>{$t("obsWs.hors.l3")}</li>
        <li>{$t("obsWs.hors.l4")}</li>
      </ul>
    </div>
  </div>
</Modal>

<style>
  .obsws-body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 0.8rem 1rem;
    display: flex;
    flex-direction: column;
    gap: 0.7rem;
  }
  .ws-para {
    margin: 0;
    font-size: 0.8rem;
    line-height: 1.45;
    opacity: 0.9;
  }
  .etat-ligne {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }
  .etat-texte {
    font-size: 0.82rem;
    font-weight: 600;
  }
  .etat-adr {
    font-size: 0.72rem;
    opacity: 0.55;
    font-family: monospace;
  }
  .feat-groupe {
    font-size: 0.68rem;
    font-weight: 700;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    opacity: 0.6;
    margin-top: 0.35rem;
  }
  .feat-ligne {
    display: flex;
    align-items: center;
    gap: 0.55rem;
    padding: 0.35rem 0.4rem;
    border-radius: var(--rayon-petit);
    background: var(--btn-surface);
    box-shadow: var(--btn-inset);
  }
  /* Pastille d'état : vert sémantique quand la fonctionnalité est en
     service, gris sinon — même convention que .dot.on de la Toolbar. */
  .feat-dot {
    flex: none;
    width: 0.5rem;
    height: 0.5rem;
    border-radius: 999px;
    background: var(--gris, #9a9aa5);
    opacity: 0.6;
  }
  .feat-dot.on {
    background: var(--message-ok-color);
    opacity: 1;
    box-shadow: 0 0 6px var(--message-ok-color);
  }
  .feat-txt {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 0.1rem;
  }
  .feat-nom {
    font-size: 0.8rem;
    font-weight: 600;
  }
  .feat-desc {
    font-size: 0.7rem;
    opacity: 0.65;
    line-height: 1.35;
  }
  .feat-etat {
    flex: none;
    font-size: 0.62rem;
    font-weight: 700;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    opacity: 0.55;
  }
  .feat-etat.on {
    color: var(--message-ok-color);
    opacity: 1;
  }
  .ws-liste {
    margin: 0;
    padding-left: 1.1rem;
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    font-size: 0.78rem;
    opacity: 0.85;
    line-height: 1.4;
  }
  /* .bloc-titre / .toggle = recettes locales des autres modales
     (ModulesModal / ModerationModal — pas de classe globale). */
  .bloc-titre {
    font-size: 0.75rem;
    font-weight: 700;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    opacity: 0.8;
  }
  .toggle {
    display: flex;
    align-items: center;
    gap: 0.45rem;
    font-size: 0.82rem;
    cursor: pointer;
  }
  .toggle input[type="checkbox"] {
    accent-color: var(--accent-ui);
  }
</style>
