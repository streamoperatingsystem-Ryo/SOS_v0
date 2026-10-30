<script lang="ts">
  // Modale « Sortie » du moteur natif — équivalent du panneau Sortie d'OBS.
  //
  // Trois blocs :
  //   1. Vidéo : résolution de sortie (canvas ou hauteur réduite — le
  //      rescale GPU se fait dans la passe NV12, la composition reste à
  //      la taille du canvas), conteneur, FPS, codec, débit, preset —
  //      persistés dans moteur.json (bloc `sortie`), consommés au
  //      démarrage d'un enregistrement ET d'un stream.
  //   2. Audio : loopback + micro, gains en % (millièmes côté backend).
  //   3. Stream : protocole RTMP/SRT + URL + clé — démarre le flux natif
  //      (même session qu'un enregistrement : arrêt commun, état commun).
  //      La clé et la passphrase vivent dans le keyring OS, jamais en clair.
  import { onMount } from "svelte";
  import Modal from "./Modal.svelte";
  import { t } from "../stores/langue";
  import type { ApercuNatifCfg, SortieNatifCfg } from "../tauri";
  import {
    moteurErreur,
    apercuCfg,
    sortieCfg,
    sortieNatif,
    sortieNatifBusy,
    streamCfgPersistee,
    chargerApercuCfg,
    sauverApercuCfg,
    chargerSortieCfg,
    sauverSortieCfg,
    chargerStreamConfig,
    demarrerStreamNatif,
    arreterEnregistrementNatif,
  } from "../stores/moteur";
  import { sortieNatifModalOpen } from "../stores/ui";
  import { sceneStore } from "../stores/scene";

  function onFermer() {
    sortieNatifModalOpen.set(false);
  }

  // ===== État local du formulaire (miroir des stores, sauvegardé au change) =====
  let codec = $state("h264");
  let conteneur = $state("mp4");
  let resolution = $state<number | null>(null); // null = canvas
  let debit = $state(6000);
  let fps = $state(60);
  let preset = $state("equilibre");
  let loopback = $state(true);
  let micro = $state(true);
  let gainLb = $state(100); // %
  let gainMic = $state(100); // %

  // Projecteur natif (aperçu réduit)
  let projLargeur = $state(1280);
  let projFps = $state(30);

  // Stream
  let protocole = $state("rtmp");
  let url = $state("");
  let cle = $state("");
  let srtPass = $state("");
  let srtLat = $state(3000);

  let sauveErr = $state<string | null>(null);
  let sauveOk = $state(false);

  let resolutionCanvas = $derived(`${$sceneStore.canvasW}×${$sceneStore.canvasH}`);
  let sortieActive = $derived($sortieNatif.actif);
  let busy = $derived($sortieNatifBusy);

  const CODECS = ["h264", "hevc", "av1"];
  const RES_HAUTEURS = [720, 540, 480, 360];
  const FPS_CHOIX = [24, 30, 50, 60, 120];
  const PRESETS = ["rapide", "equilibre", "qualite"];
  const PRESET_CLES: Record<string, string> = {
    rapide: "sortieModal.presetRapide",
    equilibre: "sortieModal.presetEquilibre",
    qualite: "sortieModal.presetQualite",
  };
  const PROJ_LARGEURS = [480, 960, 1280, 0]; // 0 = résolution canvas
  const PROJ_FPS_CHOIX = [10, 15, 30];

  onMount(async () => {
    await chargerSortieCfg();
    const s = $sortieCfg;
    codec = CODECS.includes(s.codec) ? s.codec : "h264";
    conteneur = s.conteneur === "mkv" ? "mkv" : "mp4";
    resolution =
      s.resolution !== null && RES_HAUTEURS.includes(s.resolution) ? s.resolution : null;
    debit = s.debit_kbps;
    fps = FPS_CHOIX.includes(s.fps) ? s.fps : 60;
    preset = PRESETS.includes(s.preset) ? s.preset : "equilibre";
    loopback = s.audio_loopback;
    micro = s.audio_micro;
    gainLb = Math.round(s.gain_loopback / 10);
    gainMic = Math.round(s.gain_micro / 10);
    await chargerApercuCfg();
    const a = $apercuCfg;
    projLargeur = PROJ_LARGEURS.includes(a.largeur) ? a.largeur : 1280;
    projFps = PROJ_FPS_CHOIX.includes(a.fps) ? a.fps : 30;
    await chargerStreamConfig();
    const st = $streamCfgPersistee;
    if (st) {
      protocole = st.protocole === "srt" ? "srt" : "rtmp";
      url = st.url;
      cle = st.cle ?? "";
      srtPass = st.srt_passphrase ?? "";
      srtLat = st.srt_latency_ms ?? 3000;
    }
  });

  /// Construit la config persistée depuis l'état local — gains % → millièmes.
  function cfgCourante(): SortieNatifCfg {
    return {
      codec,
      conteneur,
      resolution,
      debit_kbps: Math.max(0, Math.round(debit)),
      fps,
      preset,
      audio_loopback: loopback,
      audio_micro: micro,
      gain_loopback: Math.round(gainLb * 10),
      gain_micro: Math.round(gainMic * 10),
    };
  }

  /// Persiste à chaque changement (pattern des autres modales : pas de
  /// bouton « Appliquer », le réglage est effectif au prochain démarrage).
  async function sauver(): Promise<void> {
    sauveErr = null;
    sauveOk = false;
    try {
      await sauverSortieCfg(cfgCourante());
      sauveOk = true;
    } catch (e) {
      sauveErr = String(e);
    }
  }

  /// Persiste les réglages du projecteur — le backend relance une session
  /// aperçu en cours, l'effet est quasi immédiat.
  async function sauverProj(): Promise<void> {
    sauveErr = null;
    sauveOk = false;
    const cfg: ApercuNatifCfg = { largeur: projLargeur, fps: projFps };
    try {
      await sauverApercuCfg(cfg);
      sauveOk = true;
    } catch (e) {
      sauveErr = String(e);
    }
  }

  async function onDemarrerStream(): Promise<void> {
    await demarrerStreamNatif({
      protocole,
      url: url.trim(),
      cle: cle.trim() || undefined,
      srt_passphrase: protocole === "srt" ? srtPass.trim() || undefined : undefined,
      srt_latency_ms: protocole === "srt" && srtLat > 0 ? Math.round(srtLat) : undefined,
    });
  }
</script>

<Modal
  title={$t("sortieModal.titre")}
  onClose={onFermer}
  maxWidth="40rem"
  hint={$t("sortieModal.hint")}
  crochet="var(--crochet-dore)"
>
  <div class="modal-scroll">
    <!-- Vidéo : résolution, conteneur, fps, codec, débit, preset. -->
    <div class="modal-bloc">
      <div class="modal-bloc-titre">{$t("sortieModal.blocVideo")}</div>
      <div class="champ-grid">
        <div class="field">
          <span class="field-label">{$t("sortieModal.resolution")}</span>
          <select bind:value={resolution} onchange={sauver}>
            <option value={null}>{$t("sortieModal.resCanvas", { r: resolutionCanvas })}</option>
            {#each RES_HAUTEURS as h (h)}
              {@const w = (Math.round((($sceneStore.canvasW ?? 1920) * h) / Math.max(1, $sceneStore.canvasH ?? 1080)) + 1) & ~1}
              <option value={h}>{w}×{h}</option>
            {/each}
          </select>
        </div>
        <div class="field">
          <span class="field-label">{$t("sortieModal.conteneur")}</span>
          <select bind:value={conteneur} onchange={sauver}>
            <option value="mp4">MP4</option>
            <option value="mkv">MKV</option>
          </select>
        </div>
        <div class="field">
          <span class="field-label">{$t("sortieModal.fps")}</span>
          <select bind:value={fps} onchange={sauver}>
            {#each FPS_CHOIX as f (f)}
              <option value={f}>{f}</option>
            {/each}
          </select>
        </div>
        <div class="field">
          <span class="field-label">{$t("sortieModal.codec")}</span>
          <select bind:value={codec} onchange={sauver}>
            <option value="h264">H.264</option>
            <option value="hevc">HEVC</option>
            <option value="av1">AV1</option>
          </select>
        </div>
        <div class="field">
          <span class="field-label">{$t("sortieModal.debit")}</span>
          <input type="number" min="500" max="200000" step="100" bind:value={debit} onchange={sauver} />
        </div>
        <div class="field">
          <span class="field-label">{$t("sortieModal.preset")}</span>
          <select bind:value={preset} onchange={sauver}>
            {#each PRESETS as p (p)}
              <option value={p}>{$t(PRESET_CLES[p])}</option>
            {/each}
          </select>
        </div>
      </div>
      <p class="hint">{$t("sortieModal.resolutionHint", { r: resolutionCanvas })}</p>
      {#if sauveErr}
        <p class="hint msg-err">{sauveErr}</p>
      {:else if sauveOk}
        <p class="hint msg-ok">{$t("sortieModal.sauve")}</p>
      {/if}
    </div>

    <!-- Audio : loopback + micro, gains en % (millièmes côté backend). -->
    <div class="modal-bloc">
      <div class="modal-bloc-titre">{$t("sortieModal.blocAudio")}</div>
      <label class="modal-toggle">
        <input type="checkbox" bind:checked={loopback} onchange={sauver} />
        <span>{$t("sortieModal.loopback")}</span>
      </label>
      {#if loopback}
        <div class="gain-ligne">
          <span class="field-label">{$t("sortieModal.gain")} — {gainLb} %</span>
          <input class="modal-range" type="range" min="0" max="200" step="5" bind:value={gainLb} onchange={sauver} />
        </div>
      {/if}
      <label class="modal-toggle">
        <input type="checkbox" bind:checked={micro} onchange={sauver} />
        <span>{$t("sortieModal.micro")}</span>
      </label>
      {#if micro}
        <div class="gain-ligne">
          <span class="field-label">{$t("sortieModal.gain")} — {gainMic} %</span>
          <input class="modal-range" type="range" min="0" max="200" step="5" bind:value={gainMic} onchange={sauver} />
        </div>
      {/if}
    </div>

    <!-- Projecteur : qualité et cadence de l'aperçu réduit (moteur.json). -->
    <div class="modal-bloc">
      <div class="modal-bloc-titre">{$t("sortieModal.blocProjecteur")}</div>
      <div class="champ-grid">
        <div class="field">
          <span class="field-label">{$t("sortieModal.projQualite")}</span>
          <select bind:value={projLargeur} onchange={sauverProj}>
            {#each PROJ_LARGEURS as l (l)}
              <option value={l}>
                {l === 0 ? $t("sortieModal.projCanvas", { r: resolutionCanvas }) : `${l} px`}
              </option>
            {/each}
          </select>
        </div>
        <div class="field">
          <span class="field-label">{$t("sortieModal.projFps")}</span>
          <select bind:value={projFps} onchange={sauverProj}>
            {#each PROJ_FPS_CHOIX as f (f)}
              <option value={f}>{f}</option>
            {/each}
          </select>
        </div>
      </div>
      <p class="hint">{$t("sortieModal.projHint")}</p>
    </div>

    <!-- Stream : RTMP/SRT vers une URL + clé (keyring, jamais en clair). -->
    <div class="modal-bloc">
      <div class="modal-bloc-titre">{$t("sortieModal.blocStream")}</div>
      <p class="hint">{$t("sortieModal.streamHint")}</p>
      <div class="champ-grid">
        <div class="field">
          <span class="field-label">{$t("sortieModal.protocole")}</span>
          <select bind:value={protocole} disabled={sortieActive || busy}>
            <option value="rtmp">RTMP</option>
            <option value="srt">SRT</option>
          </select>
        </div>
        <div class="field champ-large">
          <span class="field-label">{$t("sortieModal.url")}</span>
          <input
            bind:value={url}
            disabled={sortieActive || busy}
            placeholder={protocole === "srt" ? "srt://hote:port" : "rtmp://hote/app"}
            spellcheck="false"
          />
        </div>
        <div class="field champ-large">
          <span class="field-label">{$t("sortieModal.cle")}</span>
          <input
            type="password"
            bind:value={cle}
            disabled={sortieActive || busy}
            spellcheck="false"
          />
        </div>
        {#if protocole === "srt"}
          <div class="field">
            <span class="field-label">{$t("sortieModal.srtPass")}</span>
            <input type="password" bind:value={srtPass} disabled={sortieActive || busy} spellcheck="false" />
          </div>
          <div class="field">
            <span class="field-label">{$t("sortieModal.srtLat")}</span>
            <input type="number" min="20" max="8000" step="20" bind:value={srtLat} disabled={sortieActive || busy} />
          </div>
        {/if}
      </div>
      {#if sortieActive && $sortieNatif.chemin}
        <p class="hint msg-ok">{$t("sortieModal.enCours", { cible: $sortieNatif.chemin })}</p>
      {/if}
      {#if $moteurErreur}
        <p class="hint msg-err">{$moteurErreur}</p>
      {/if}
      <div class="modal-actions">
        {#if sortieActive}
          <button class="action" onclick={() => arreterEnregistrementNatif()} disabled={busy}>
            {$t("sortieModal.arreter")}
          </button>
        {:else}
          <button
            class="action"
            onclick={onDemarrerStream}
            disabled={busy || !url.trim()}
          >
            {$t("sortieModal.demarrer")}
          </button>
        {/if}
      </div>
    </div>
  </div>
</Modal>

<style>
  .champ-grid {
    display: grid;
    grid-template-columns: repeat(2, 1fr);
    gap: 0.5rem 0.8rem;
  }
  .champ-large {
    grid-column: span 2;
  }
  .gain-ligne {
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
    padding-left: 1.2rem;
  }
</style>
