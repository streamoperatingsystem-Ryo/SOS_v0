<script lang="ts">
  import { onDestroy } from "svelte";
  import { get } from "svelte/store";
  import { tauri, type CaptureCible } from "../tauri";
  import { createObsTrouFromPc, lierFondCapture } from "../stores/scene";
  import Modal from "./Modal.svelte";
  import { t } from "../stores/langue";

  // mode "widget" (défaut) : source liée au widget trou sélectionné (widgetId).
  // mode "fond" : capture plein canvas comme FOND de la scène (pas de
  // widgetId — source "SOS-Fond-<sceneId8>" gérée par lierFondCapture). La
  // caméra n'est PAS proposée en mode fond : conflit d'accès exclusif dshow
  // avec un éventuel widget caméra sur le même device.
  let {
    widgetId,
    mode = "widget",
    onClose,
  }: { widgetId?: string; mode?: "widget" | "fond"; onClose: () => void } = $props();

  // 2 étapes : "type" (boutons) → "liste" (grille de cartes avec miniatures
  // WGC — même API qu'OBS, la vignette montre ce qui sera capturé) → "working".
  type Etape = "type" | "liste" | "working";
  type Kind = "camera" | "window" | "game" | "monitor";
  let etape = $state<Etape>("type");
  let kind = $state<Kind | null>(null);

  // Item générique de la grille :
  // - caméra : nom device PnP (obs_value null → cible = nom), pas de
  //   miniature (conflit d'accès exclusif dshow avec OBS) — emoji 📷.
  // - fenêtre/jeu : titre + exe + obs_value ("title:class:exe") + hwnd
  //   (miniature WGC à la demande, fallback icône de fenêtre côté Rust).
  // - écran : "Écran N" + résolution + obs_value = monitor_id OBS + hmon
  //   (miniature WGC du bureau).
  type Item = {
    nom: string;
    sub: string;
    obs_value: string | null;
    hwnd?: number;
    hmon?: number;
    /// Index DXGI du moniteur (cible du moteur natif).
    index?: number;
  };
  let liste = $state<Item[]>([]);
  // Miniatures à la demande : clé = obs_value (ou nom) → data URL PNG.
  // "" = chargé sans image (→ emoji) ; clé absente = chargement en cours.
  let thumbs = $state<Record<string, string>>({});
  let filtre = $state("");
  let loading = $state(false);
  let error = $state<string | null>(null);
  let working = $state(false);
  let ferme = false;
  onDestroy(() => (ferme = true));

  const cleItem = (i: Item) => i.obs_value ?? i.nom;

  const EMOJI: Record<Kind, string> = {
    camera: "📷",
    window: "🪟",
    game: "🎮",
    monitor: "🖥️",
  };

  // Étape 1 → Étape 2 : charge la liste vivante PC (pas GetInputList), puis
  // lance les miniatures en arrière-plan (les cartes se remplissent en live).
  async function pickType(k: Kind) {
    kind = k;
    etape = "liste";
    loading = true;
    error = null;
    filtre = "";
    liste = [];
    thumbs = {};
    try {
      if (k === "camera") {
        const devices = await tauri.pcEnumerateCameras();
        liste = devices.map((d) => ({ nom: d, sub: "", obs_value: null }));
      } else if (k === "monitor") {
        const mons = await tauri.pcEnumerateMonitors();
        const trad = get(t);
        liste = mons.map((m) => ({
          nom: trad("obsSourceDialog.ecranN", { n: m.index + 1 }),
          sub:
            `${m.w}×${m.h}` +
            (m.primary ? " · " + trad("obsSourceDialog.principal") : ""),
          // monitor_id vide (EnumDisplayDevices ko) → null = moniteur OBS
          // par défaut plutôt qu'un nom localisé sans valeur pour OBS.
          obs_value: m.monitor_id || null,
          hmon: m.hmon,
          index: m.index,
        }));
      } else {
        const wins =
          k === "window"
            ? await tauri.pcEnumerateWindows()
            : await tauri.pcEnumerateGames();
        liste = wins
          .map((w) => ({ nom: w.title, sub: w.exe, obs_value: w.obs_value, hwnd: w.hwnd }))
          .sort((a, b) => a.nom.localeCompare(b.nom, undefined, { sensitivity: "base" }));
      }
    } catch (e) {
      error = String((e as Error)?.message ?? e);
    } finally {
      loading = false;
    }
    chargerMiniatures();
  }

  // Miniatures à la demande : un invoke par carte — WGC_LOCK sérialise les
  // sessions côté Rust, les cartes se remplissent progressivement.
  function chargerMiniatures() {
    for (const item of liste) {
      const cle = cleItem(item);
      const req =
        item.hwnd != null
          ? tauri.pcWindowThumbnail(item.hwnd)
          : item.hmon != null
            ? tauri.pcMonitorThumbnail(item.hmon)
            : null;
      if (!req) {
        thumbs[cle] = ""; // caméra : jamais de miniature
        continue;
      }
      req
        .then((url) => {
          if (!ferme) thumbs[cle] = url ?? "";
        })
        .catch(() => {
          if (!ferme) thumbs[cle] = "";
        });
    }
  }

  // Filtre insensible à la casse sur le titre et la sous-ligne (exe).
  const listeFiltree = $derived.by(() => {
    const f = filtre.trim().toLowerCase();
    if (!f) return liste;
    return liste.filter(
      (i) => i.nom.toLowerCase().includes(f) || i.sub.toLowerCase().includes(f)
    );
  });

  // Clic carte → créer la source OBS du widget, ou lier la capture de fond.
  async function onPick(item: Item) {
    if (!kind) return;
    // target : caméra = nom device ; fenêtre/jeu = obs_value
    // ("title:class:exe") ; écran = monitor_id (null = écran par défaut OBS).
    const target = item.obs_value ?? (kind === "monitor" ? null : item.nom);
    // Cible persistée pour le moteur natif (absente = mode OBS seul).
    // Caméra exclue : Media Foundation n'existe pas encore côté moteur.
    const cible: CaptureCible | undefined =
      kind === "monitor"
        ? { kind: "moniteur", name: item.obs_value ?? undefined, index: item.index ?? 0 }
        : (kind === "window" || kind === "game") && item.hwnd != null
          ? { kind: "fenetre", hwnd: item.hwnd, title: item.nom, exe: item.sub }
          : undefined;
    etape = "working";
    working = true;
    error = null;
    try {
      if (mode === "fond") {
        await lierFondCapture(kind, target);
      } else {
        if (!widgetId) return;
        await createObsTrouFromPc(widgetId, kind, target, cible);
      }
      onClose();
    } catch (e) {
      const msg = String((e as Error)?.message ?? e);
      error = msg;
      etape = "liste";
    } finally {
      working = false;
    }
  }

  function onBack() {
    etape = "type";
    kind = null;
    liste = [];
    thumbs = {};
    filtre = "";
    error = null;
  }
</script>

<Modal
  title={mode === "fond" ? $t("obsSourceDialog.titreFond") : $t("obsSourceDialog.titreWidget")}
  onClose={onClose}
  crochet="var(--crochet-dore)"
  maxWidth="56rem"
  hint={mode === "fond"
    ? $t("obsSourceDialog.hintFond")
    : $t("obsSourceDialog.hintWidget")}
>
  <div class="dialog-body">
    {#if etape === "type"}
      <p class="hint">{$t("obsSourceDialog.choisirType")}</p>
      <div class="type-buttons">
        {#if mode !== "fond"}
          <button class="action centre" onclick={() => pickType("camera")}>📷 {$t("obsSourceDialog.kind.camera")}</button>
        {/if}
        <button class="action centre" onclick={() => pickType("window")}>🪟 {$t("obsSourceDialog.kind.window")}</button>
        <button class="action centre" onclick={() => pickType("game")}>🎮 {$t("obsSourceDialog.kind.game")}</button>
        <button class="action centre" onclick={() => pickType("monitor")}>🖥️ {$t("obsSourceDialog.kind.monitor")}</button>
      </div>

    {:else if etape === "liste" || etape === "working"}
      <div class="list-header">
        <button class="action small" onclick={onBack} disabled={working}>← {$t("obsSourceDialog.retour")}</button>
        <span class="list-title">{$t("obsSourceDialog.kind." + kind)}</span>
        <input
          class="filtre"
          type="text"
          placeholder={$t("obsSourceDialog.filtrer")}
          bind:value={filtre}
        />
      </div>

      {#if loading}
        <p class="hint">{$t("commun.chargement")}</p>
      {:else if liste.length === 0}
        <p class="hint msg-warn">{$t("obsSourceDialog.aucuneCible")}</p>
      {:else}
        <div class="thumb-grid">
          {#each listeFiltree as item, i (cleItem(item) + i)}
            <button
              class="action thumb-card"
              onclick={() => onPick(item)}
              disabled={working}
            >
              <span class="thumb-frame">
                {#if thumbs[cleItem(item)]}
                  <img src={thumbs[cleItem(item)]} alt="" draggable="false" />
                {:else if cleItem(item) in thumbs}
                  <span class="thumb-fallback">{EMOJI[kind ?? "window"]}</span>
                {:else}
                  <span class="thumb-fallback pulse">{EMOJI[kind ?? "window"]}</span>
                {/if}
              </span>
              <span class="thumb-title" title={item.nom}>{item.nom}</span>
              {#if item.sub}
                <span class="thumb-sub" title={item.sub}>{item.sub}</span>
              {/if}
            </button>
          {/each}
        </div>
      {/if}

      {#if error}
        <p class="hint msg-warn">{error}</p>
      {/if}
    {/if}

    <button class="action ghost" onclick={onClose}>{$t("commun.annuler")}</button>
  </div>
</Modal>

<style>
  .dialog-body {
    padding: 1rem 1.2rem;
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
  }
  .type-buttons {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(9rem, 1fr));
    gap: 0.4rem;
  }
  .list-header {
    display: flex;
    align-items: center;
    gap: 0.6rem;
  }
  .list-title {
    font-size: 0.85rem;
    text-transform: capitalize;
    opacity: 0.8;
    flex-shrink: 0;
  }
  .filtre {
    flex: 1;
    min-width: 0;
  }

  /* Grille de cartes : auto-fill → le nombre de colonnes suit la largeur
     réelle de la modale (rem → scale 2K/4K via tailleEcran, comme tout le
     chrome). align-content:start évite l'étirement vertical des cartes. */
  .thumb-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(9.5rem, 1fr));
    gap: 0.5rem;
    overflow-y: auto;
    max-height: 24rem;
    padding-right: 0.2rem;
    align-content: start;
  }
  .thumb-card {
    display: flex;
    flex-direction: column;
    align-items: stretch;
    gap: 0.3rem;
    padding: 0.4rem;
    min-width: 0;
  }
  .thumb-frame {
    aspect-ratio: 16 / 10;
    border-radius: var(--rayon-petit);
    background: rgba(0, 0, 0, 0.35);
    border: 1px solid var(--bordure-ui);
    display: flex;
    align-items: center;
    justify-content: center;
    overflow: hidden;
  }
  .thumb-frame img {
    width: 100%;
    height: 100%;
    object-fit: contain;
  }
  .thumb-fallback {
    font-size: 1.4rem;
    opacity: 0.5;
  }
  .thumb-fallback.pulse {
    animation: thumb-pulse 1.2s ease-in-out infinite;
  }
  @keyframes thumb-pulse {
    50% {
      opacity: 0.15;
    }
  }
  .thumb-title {
    font-size: 0.78rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .thumb-sub {
    font-size: 0.68rem;
    opacity: 0.55;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
