// Store speedrun côté dashboard. Écoute l'event Tauri speedrun:event et expose
// l'état du timer, des splits, de la connexion processus et des erreurs.
// Pattern identique à chat.ts : stores writables + initSpeedrun() au montage.
import { writable, get } from "svelte/store";
import { listen, emit } from "@tauri-apps/api/event";
import { tauri, type SpeedrunEvent, type SpeedrunConfig, type AslSettingDetail } from "../tauri";
import { speedrunConfigModalOpen } from "./ui";
import { t, langue, localeTag } from "./langue";

// ===== Stores exposés au dashboard =====

/// Phase du timer : "NotRunning" | "Running" | "Paused" | "Ended".
export const phaseTimer = writable<string>("NotRunning");

/// Temps de jeu (game time). null si pas de méthode gameTime dans l'ASL.
export const tempsJeu = writable<string | null>(null);

/// Temps réel (RTA). Fallback pour tempsJeu si gameTime est null.
export const tempsReel = writable<string>("00:00.00");

/// Index du split courant (0-based). null si pas encore démarré.
export const indexSplitCourant = writable<number | null>(null);

/// Nombre total de splits.
export const totalSplits = writable<number>(0);

/// Liste des durées de segment enregistrées (String formaté, index = segment
/// 0-based). Vidée par started/reset/goto/stopped — retry de segment ou
/// nouveau run = les précédents ne comptent pas comme finis.
export const splits = writable<string[]>([]);

/// Cumuls de run aux splits réalisés (String formaté "MM:SS.mmm", index =
/// segment 0-based). Base du delta cumulé (avance/retard de la run vs PB —
/// référence LiveSplit) : rempli par event.cumul sur split/ended et par
/// splitCumuls sur etat (resync), vidé par started/reset/goto/stopped.
export const cumulsSplits = writable<(string | null)[]>([]);

/// Temps vivant du segment courant (event `time.segmentTime`, formaté).
/// null quand le timer ne tourne pas (la ligne armée affiche 0:00.00).
export const tempsSegment = writable<string | null>(null);

/// Segments LSS chargés (nom + PB). Vide si pas de .lss chargé.
/// pbRealTime/pbGameTime sont des secondes cumulées (f64 Rust), pas des strings.
export const segmentsLss = writable<{ nom: string; pbRealTime: number | null; pbGameTime: number | null }[]>([]);

/// Nom du jeu (depuis le .lss). Vide si pas de .lss.
export const nomJeuLss = writable<string>("");

/// Nom de la catégorie (depuis le .lss). Vide si pas de .lss.
export const nomCategorieLss = writable<string>("");

/// True si le splitter est actif (boucle de polling en cours).
export const estActif = writable<boolean>(false);

/// True si le processus jeu est connecté (scan mémoire actif).
export const estConnecte = writable<boolean>(false);

/// Nom du processus jeu connecté (ex: "mgsi.exe"). null si déconnecté.
export const nomProcessus = writable<string | null>(null);

/// Chemin du fichier .asl chargé. null si aucun.
export const cheminAsl = writable<string | null>(null);

/// Chemin du fichier .lss chargé. null si aucun.
export const cheminLss = writable<string | null>(null);

/// Settings ASL (quelles méthodes auto-split sont activées).
export const settingsAsl = writable<{ start: boolean; split: boolean; reset: boolean }>({
  start: true,
  split: true,
  reset: true,
});

/// Settings ASL individuels (120+ pour MGS). Chaque entrée est un setting
/// "code-signature" (ex: "OL-s00a" → "Dock") avec sa valeur cochée.
/// Vide pour les ASL simples (type SOR) qui n'ont pas de settings individuels.
export const settingsDetaillesAsl = writable<AslSettingDetail[]>([]);

/// Dictionnaire code→nom lisible (D.Names.Split de l'ASL).
/// Ex: {"OL-s00a": "Dock", "OL-s01a.CL-s02a.CP-18": "Heliport", ...}
/// Utilisé par la modale de config pour mapper les segments LSS vers les codes.
export const nomsSplitsAsl = writable<Record<string, string>>({});

/// Erreur speedrun (affichage discret). null = pas d'erreur.
export const erreurSpeedrun = writable<string | null>(null);

/// Compteur de tentatives de run — « Partie n°N » affiché dans l'export
/// splits HTML. Restauré depuis speedrun.json (nb_tentatives) au boot par
/// chargerPreferencesSpeedrun, +1 sur chaque event `started` (start manuel
/// ou ASL ; undo/goto/reset ne comptent PAS comme nouvelle tentative).
export const tentativeRun = writable<number>(0);

// ===== Init =====

// Unlisteners de TOUS les events Tauri enregistrés par initSpeedrun —
// collectés pour le cleanup HMR : en dev, chaque ré-évaluation à chaud du
// module empile sinon un nouveau jeu de listeners backend (les anciens
// ne sont jamais désenregistrés) → chaque action du pop-out s'exécute N
// fois (dialog « Enregistrer sous » ouvert en boucle sur un seul clic
// Export, observé 2026-10-21). Prod : import.meta.hot est undefined.
let unlistenAll: (() => void)[] | null = null;

/// Initialise le store : écoute l'event Tauri speedrun:event et met à jour
/// les stores. Idempotent — safe d'appeler plusieurs fois.
export async function initSpeedrun() {
  if (unlistenAll) return;
  unlistenAll = [];
  unlistenAll.push(await listen<SpeedrunEvent>("speedrun:event", (e) => {
    const event = e.payload;
    switch (event.type) {
      case "started":
        phaseTimer.set("Running");
        estActif.set(true);
        // index = split de départ (armé par clic en NotRunning, sinon 0).
        indexSplitCourant.set(event.index ?? 0);
        splits.set([]);
        cumulsSplits.set([]);
        tempsSegment.set(null);
        // « Partie n°N » : une nouvelle tentative démarre → +1, persisté
        // dans speedrun.json (read-modify-write, cf. persisterTentative).
        tentativeRun.update((n) => {
          const v = n + 1;
          persisterTentative(v);
          return v;
        });
        break;
      case "split":
        if (event.index !== undefined) {
          indexSplitCourant.set(event.index);
        }
        if (event.time && event.index !== undefined) {
          // event.index = current_split_index après incrément → le segment
          // bouclé est à 0-based = index − 1. event.time = durée du segment
          // (le cumul de run voyage dans event.cumul).
          const segIdx = event.index - 1;
          splits.update((s) => {
            while (s.length <= segIdx) s.push("");
            s[segIdx] = event.time!;
            return s;
          });
        }
        // Cumul de run au split réalisé (base du delta cumulé LiveSplit).
        if (event.cumul && event.index !== undefined) {
          const segIdx = event.index - 1;
          cumulsSplits.update((c) => {
            while (c.length <= segIdx) c.push(null);
            c[segIdx] = event.cumul!;
            return c;
          });
        }
        break;
      case "splitSkipped":
        if (event.index !== undefined) {
          indexSplitCourant.set(event.index);
        }
        break;
      case "splitUndone":
        if (event.index !== undefined) {
          // event.index = current_split_index après undo (déjà décrémenté).
          // Le split annulé est à l'index 0-based = event.index.
          indexSplitCourant.set(event.index);
          splits.update((s) => {
            if (event.index! < s.length) s[event.index!] = "";
            return s;
          });
          cumulsSplits.update((c) => {
            if (event.index! < c.length) c[event.index!] = null;
            return c;
          });
        }
        break;
      case "reset":
        phaseTimer.set("NotRunning");
        indexSplitCourant.set(null);
        splits.set([]);
        cumulsSplits.set([]);
        tempsJeu.set(null);
        tempsReel.set("00:00.00");
        tempsSegment.set(null);
        break;
      case "ended":
        phaseTimer.set("Ended");
        // Le DERNIER split émet "ended" (pas "split") côté moteur —
        // event.time = durée du dernier segment, event.index = total.
        if (event.index !== undefined) indexSplitCourant.set(event.index);
        if (event.time) {
          const segIdx = (event.index ?? get(totalSplits)) - 1;
          if (segIdx >= 0) {
            splits.update((s) => {
              while (s.length <= segIdx) s.push("");
              s[segIdx] = event.time!;
              return s;
            });
          }
        }
        if (event.cumul) {
          const segIdx = (event.index ?? get(totalSplits)) - 1;
          if (segIdx >= 0) {
            cumulsSplits.update((c) => {
              while (c.length <= segIdx) c.push(null);
              c[segIdx] = event.cumul!;
              return c;
            });
          }
        }
        break;
      case "goto":
        // Clic sur une ligne : armement (NotRunning) ou retry de segment.
        // Tous les temps de la tentative sont effacés — les précédents ne
        // comptent pas comme finis (le PB reste affiché via segmentsLss).
        if (event.phase) phaseTimer.set(event.phase);
        if (event.index !== undefined) indexSplitCourant.set(event.index);
        splits.set([]);
        cumulsSplits.set([]);
        tempsSegment.set(null);
        break;
      case "etat":
        // Resync complet (reload/reconnexion) : remplace tout l'état.
        if (event.phase) phaseTimer.set(event.phase);
        indexSplitCourant.set(
          event.index !== undefined && event.index >= 0 ? event.index : null
        );
        splits.set((event.splitTimes ?? []).map((t) => t ?? ""));
        cumulsSplits.set((event.splitCumuls ?? []).map((t) => t ?? null));
        if (event.realTime) tempsReel.set(event.realTime);
        tempsJeu.set(event.gameTime ?? event.realTime ?? null);
        tempsSegment.set(event.segmentTime ?? null);
        break;
      case "gameTime":
        if (event.time) tempsJeu.set(event.time);
        break;
      case "time":
        if (event.realTime) tempsReel.set(event.realTime);
        if (event.gameTime !== undefined) tempsJeu.set(event.gameTime);
        else if (event.realTime) tempsJeu.set(event.realTime);
        if (event.segmentTime !== undefined) tempsSegment.set(event.segmentTime);
        break;
      case "connected":
        estConnecte.set(true);
        if (event.process) nomProcessus.set(event.process);
        break;
      case "disconnected":
        // Sortie du jeu : le timer Rust a reset → l'affichage suit.
        estConnecte.set(false);
        nomProcessus.set(null);
        phaseTimer.set("NotRunning");
        indexSplitCourant.set(null);
        splits.set([]);
        cumulsSplits.set([]);
        tempsJeu.set(null);
        tempsReel.set("00:00.00");
        tempsSegment.set(null);
        break;
      case "error":
        if (event.message) erreurSpeedrun.set(event.message);
        break;
      case "stopped":
        estActif.set(false);
        estConnecte.set(false);
        phaseTimer.set("NotRunning");
        indexSplitCourant.set(null);
        splits.set([]);
        cumulsSplits.set([]);
        tempsSegment.set(null);
        break;
      case "paused":
        phaseTimer.set("Paused");
        break;
      case "resumed":
        phaseTimer.set("Running");
        break;
      case "loaded":
        // ASL chargé avec succès
        break;
      case "settings-list":
        // Les settings sont gérés côté frontend (toggles), pas besoin de
        // les synchroniser depuis le backend ici.
        break;
      case "saved":
        // PB sauvegardé dans le .lss (run terminée plus rapide que le PB) :
        // recharger silencieusement le fichier pour rafraîchir les PB
        // affichés. La garde same-path côté Rust évite tout reset moteur.
        if (event.chemin) {
          chargerLss(event.chemin).catch(() => {});
        }
        break;
    }
  }));

  // ===== Pop-out speedrun (fenêtre détachable, 2026-10-19) =====
  // Pattern raccourcis : la fenêtre émet speedrun-popout:action, ce store
  // exécute (invoke Rust). Le dashboard pousse l'état moteur à la fenêtre
  // via speedrun-popout:etat — le WS ne dit pas si le moteur tourne quand
  // le timer est NotRunning armé (start/stop des boutons du pop-out).
  // Handshake speedrun-popout:ready (listeners prêts → push immédiat, même
  // mécanique que raccourcis:ready). Tout est non-fatal sans la fenêtre.
  const pousserEtatPopout = () => {
    emit("speedrun-popout:etat", { actif: get(estActif) }).catch(() => {});
  };
  estActif.subscribe(() => {
    pousserEtatPopout();
  });
  unlistenAll.push(await listen<{ action: string; index?: number }>("speedrun-popout:action", (e) => {
    const a = e.payload?.action;
    switch (a) {
      case "demarrer": {
        const nb = get(totalSplits) > 0 ? get(totalSplits) : get(segmentsLss).length || 1;
        demarrerSpeedrun(nb).catch(() => {});
        break;
      }
      case "arreter":
        arreterSpeedrun().catch(() => {});
        break;
      case "goto":
        if (typeof e.payload.index === "number") {
          allerSplit(e.payload.index).catch(() => {});
        }
        break;
      case "split":
      case "skip":
      case "undo":
      case "reset":
      case "pause":
        actionManuelle(a).catch(() => {});
        break;
      case "export":
        exporterSplits().catch(() => {});
        break;
    }
  }));
  unlistenAll.push(await listen("speedrun-popout:ready", () => {
    pousserEtatPopout();
  }));

  // Dev HMR : à chaque ré-évaluation à chaud de ce module, désenregistrer
  // les listeners de l'ANCIENNE instance — sinon ils s'empilent côté
  // backend (l'event part alors vers N handlers → actions du pop-out
  // exécutées N fois). No-op en prod (import.meta.hot undefined).
  const fns = unlistenAll;
  import.meta.hot?.dispose(() => {
    fns.forEach((u) => u());
    unlistenAll = null;
  });
}

/// Charge les préférences speedrun sauvegardées (speedrun.json) au boot.
/// Retourne la config pour que le widget puisse recharger les chemins.
export async function chargerPreferencesSpeedrun(): Promise<SpeedrunConfig | null> {
  try {
    const config = await tauri.speedrunLireConfig();
    if (config.chemin_asl) cheminAsl.set(config.chemin_asl);
    if (config.chemin_lss) cheminLss.set(config.chemin_lss);
    if (config.settings) settingsAsl.set(config.settings);
    tentativeRun.set(config.nb_tentatives ?? 0);
    return config;
  } catch {
    return null;
  }
}

/// Persiste le compteur de tentatives (nb_tentatives) dans speedrun.json.
/// ⚠️ READ-MODIFY-WRITE OBLIGATOIRE : sauver_speedrun_config écrase TOUT
/// le fichier — sans relecture préalable on perdrait settings_asl et les
/// chemins ASL/LSS.
async function persisterTentative(n: number) {
  try {
    const config = await tauri.speedrunLireConfig();
    config.nb_tentatives = n;
    await tauri.speedrunSauverConfig(config);
  } catch {
    // Non-fatal : le compteur sera ré-incrémenté au prochain start.
  }
}

/// Sauve les préférences speedrun (chemins ASL/LSS + settings).
/// Read-modify-write : speedrunSauverConfig écrase TOUT speedrun.json —
/// sans relecture, settings_asl et nb_tentatives seraient effacés à chaque
/// choix de fichier / toggle auto.
export async function sauverPreferencesSpeedrun(
  cheminAslVal: string | null,
  cheminLssVal: string | null,
  settings: { start: boolean; split: boolean; reset: boolean }
) {
  try {
    const config = await tauri.speedrunLireConfig();
    config.chemin_asl = cheminAslVal ?? undefined;
    config.chemin_lss = cheminLssVal ?? undefined;
    config.settings = settings;
    await tauri.speedrunSauverConfig(config);
  } catch {
    // Non-fatal
  }
}

// ===== Actions =====

/// Charge un fichier .asl et met à jour le store. Si l'ASL expose des settings
/// individuels (120+ pour MGS), ouvre automatiquement la modale de configuration
/// pour que l'utilisateur puisse cocher/décocher les splits — comme dans LiveSplit.
/// Si des settings ASL sont déjà sauvegardés dans speedrun.json, les restaure
/// au lieu de réouvrir la modale.
export async function chargerAsl(path: string) {
  try {
    const result = await tauri.speedrunChargerAsl(path);
    cheminAsl.set(path);
    // Stocker les settings de base (start/split/reset)
    const startSetting = result.settings.find((s) => s.id === "start");
    const splitSetting = result.settings.find((s) => s.id === "split");
    const resetSetting = result.settings.find((s) => s.id === "reset");
    settingsAsl.set({
      start: startSetting?.value ?? true,
      split: splitSetting?.value ?? true,
      reset: resetSetting?.value ?? true,
    });
    // Stocker les settings individuels + noms de splits pour la modale
    settingsDetaillesAsl.set(result.settings_detailles || []);
    const nomsMap: Record<string, string> = {};
    for (const [code, nom] of result.noms_splits || []) {
      nomsMap[code] = nom;
    }
    nomsSplitsAsl.set(nomsMap);
    // Si l'ASL a des settings individuels, vérifier si l'utilisateur a déjà
    // une config sauvegardée. Si oui → restaurer silencieusement. Si non →
    // ouvrir la modale de configuration (premier lancement).
    if ((result.settings_detailles?.length ?? 0) > 0) {
      const config = await tauri.speedrunLireConfig();
      const settingsSauves = config.settings_asl;
      if (settingsSauves && Object.keys(settingsSauves).length > 0) {
        // Restauration silencieuse : appliquer les settings sauvegardés au runtime
        await restaurerSettingsAsl();
      } else {
        // Premier lancement : ouvrir la modale
        speedrunConfigModalOpen.set(true);
      }
    }
    erreurSpeedrun.set(null);
  } catch (e) {
    erreurSpeedrun.set(String(e));
    throw e;
  }
}

/// Charge un fichier .lss et met à jour les stores (segments + noms).
export async function chargerLss(path: string) {
  try {
    const run = await tauri.speedrunChargerLss(path);
    cheminLss.set(path);
    nomJeuLss.set(run.nom_jeu || "");
    nomCategorieLss.set(run.nom_categorie || "");
    segmentsLss.set(
      (run.segments || []).map((s) => ({
        nom: s.nom,
        pbRealTime: s.pb_real_time,
        pbGameTime: s.pb_game_time,
      }))
    );
    totalSplits.set(run.segments?.length || 0);
    erreurSpeedrun.set(null);
  } catch (e) {
    erreurSpeedrun.set(String(e));
    throw e;
  }
}

/// Démarre le splitter avec les settings courants.
export async function demarrerSpeedrun(nbSplits: number) {
  try {
    const s = { start: true, split: true, reset: true };
    settingsAsl.subscribe((v) => Object.assign(s, v))();
    await tauri.speedrunDemarrer(nbSplits, s.start, s.split, s.reset);
    estActif.set(true);
    erreurSpeedrun.set(null);
  } catch (e) {
    erreurSpeedrun.set(String(e));
    throw e;
  }
}

/// Arrête le splitter.
export async function arreterSpeedrun() {
  try {
    await tauri.speedrunArreter();
    estActif.set(false);
    estConnecte.set(false);
    phaseTimer.set("NotRunning");
    indexSplitCourant.set(null);
    splits.set([]);
    tempsSegment.set(null);
  } catch (e) {
    erreurSpeedrun.set(String(e));
    throw e;
  }
}

/// Met à jour les settings ASL.
export async function majSettings(start: boolean, split: boolean, reset: boolean) {
  try {
    await tauri.speedrunMajSettings(start, split, reset);
    settingsAsl.set({ start, split, reset });
  } catch (e) {
    erreurSpeedrun.set(String(e));
    throw e;
  }
}

/// Clic sur une ligne de split : arme l'index si le timer est arrêté (le
/// prochain start ASL/manuel part de là), sinon saute à l'index en retry
/// de segment — chrono de segment à zéro, temps de la tentative effacés.
export async function allerSplit(index: number) {
  try {
    await tauri.speedrunAllerSplit(index);
    erreurSpeedrun.set(null);
  } catch (e) {
    erreurSpeedrun.set(String(e));
    throw e;
  }
}

/// Envoie une action manuelle : "start", "split", "skip", "undo", "reset", "pause".
export async function actionManuelle(action: string) {
  try {
    await tauri.speedrunActionManuelle(action);
    erreurSpeedrun.set(null);
  } catch (e) {
    erreurSpeedrun.set(String(e));
    throw e;
  }
}

/// Applique les settings ASL individuels (120+ pour MGS) au runtime Boa et
/// les persiste dans speedrun.json. Appelé quand l'utilisateur valide la modale
/// de configuration. `valeurs` est une map code→bool (coché/décoché).
export async function majSettingsAslEtPersister(valeurs: Record<string, boolean>) {
  try {
    // 1. Appliquer au runtime Boa (effet immédiat sur F.SettingEnabled)
    await tauri.speedrunMajSettingsAsl(valeurs);
    // 2. Mettre à jour le store local
    settingsDetaillesAsl.update((settings) =>
      settings.map((s) => ({ ...s, value: valeurs[s.id] ?? s.value }))
    );
    // 3. Persister dans speedrun.json (pour restauration au prochain boot)
    const config = await tauri.speedrunLireConfig();
    config.settings_asl = valeurs;
    await tauri.speedrunSauverConfig(config);
    erreurSpeedrun.set(null);
  } catch (e) {
    erreurSpeedrun.set(String(e));
    throw e;
  }
}

/// Restaure les settings ASL individuels sauvegardés (depuis speedrun.json)
/// dans le runtime Boa au chargement d'un ASL. Évite de réouvrir la modale
/// à chaque boot si l'utilisateur a déjà configuré les splits. Appelé après
/// chargerAsl si settings_asl est non vide dans la config.
export async function restaurerSettingsAsl() {
  try {
    const config = await tauri.speedrunLireConfig();
    if (config.settings_asl && Object.keys(config.settings_asl).length > 0) {
      await tauri.speedrunMajSettingsAsl(config.settings_asl);
      // Mettre à jour le store local avec les valeurs restaurées
      settingsDetaillesAsl.update((settings) =>
        settings.map((s) => ({
          ...s,
          value: config.settings_asl?.[s.id] ?? s.value,
        }))
      );
    }
  } catch {
    // Non-fatal : si la restauration échoue, l'utilisateur reconfigurera
  }
}

// ===== Export des splits — fichier HTML autonome (2026-10-21) =====
// Bouton « Exporter les splits » présent dans le dashboard (CarteEdition)
// ET le pop-out (action "export" → ce store exécute, convention satellite).
// Génère un .html auto-portant : CSS inline, zéro ressource externe (le
// fichier reste local — contrat vie privée — et s'ouvre hors-ligne).
// Contenu : jeu + catégorie, « Partie n°N » (nb_tentatives), statut de la
// run, temps total, tableau # · split · durée segment · temps total ·
// delta vs PB (vert/rouge) — splits skippés marqués, non atteints en PB
// muté. Exportable à tout moment : reflète l'état courant des stores.

function esc(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

/// "MM:SS.cc" / "H:MM:SS.cc" → secondes (même format que le store).
function parseTempsSr(str: string | null | undefined): number {
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

/// Secondes → "M:SS.cc" / "H:MM:SS.cc" (pour les PB du .lss, en f64).
function fmtTempsCs(secs: number): string {
  if (secs < 0 || isNaN(secs)) return "—";
  const totalCs = Math.round(secs * 100);
  const h = Math.floor(totalCs / 360000);
  const m = Math.floor((totalCs % 360000) / 6000);
  const s = Math.floor((totalCs % 6000) / 100);
  const cs = totalCs % 100;
  const base =
    h > 0
      ? `${h}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`
      : `${m}:${String(s).padStart(2, "0")}`;
  return `${base}.${String(cs).padStart(2, "0")}`;
}

/// Delta signé — format LiveSplit : "±S.d" < 60 s, "±M:SS" < 1 h,
/// "±H:MM:SS" au-delà (décimale abandonnée ≥ 1 min).
function fmtDeltaSr(secs: number): string {
  const sign = secs < 0 ? "−" : "+";
  const a = Math.abs(secs);
  if (a < 60) return `${sign}${a.toFixed(1)}`;
  const total = Math.floor(a);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  return h > 0
    ? `${sign}${h}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`
    : `${sign}${m}:${String(s).padStart(2, "0")}`;
}

/// Garde anti-réentrance : un dialog « Enregistrer sous » à la fois.
let exportEnCours = false;

/// Construit et enregistre l'export HTML via le dialog « Enregistrer sous »
/// (commande speedrun_exporter_splits — dossier + nom choisis par
/// l'utilisateur). Erreur affichée via erreurSpeedrun si aucun .lss.
export async function exporterSplits() {
  const segs = get(segmentsLss);
  const tr = get(t);
  if (segs.length === 0) {
    erreurSpeedrun.set(tr("widgetSpeedrun.exportAucun"));
    return;
  }
  const phase = get(phaseTimer);
  const idx = get(indexSplitCourant);
  const sDurees = get(splits);
  const sCumuls = get(cumulsSplits);
  const jeu = get(nomJeuLss);
  const cat = get(nomCategorieLss);
  const live = get(tempsJeu) ?? get(tempsReel);
  const segLive = get(tempsSegment);
  const n = get(tentativeRun);
  // Colonne PB dans la même base de temps que la run (game time dès que
  // l'ASL la fournit) — même règle que pbColonne côté WidgetSpeedrun.
  const baseJeu = get(tempsJeu) != null;
  const pbCol = (i: number): number | null =>
    baseJeu
      ? segs[i].pbGameTime ?? segs[i].pbRealTime
      : segs[i].pbRealTime ?? segs[i].pbGameTime;

  const celluleDelta = (cumul: number | null, pb: number | null): string => {
    if (cumul === null || pb === null) return `<td class="temps muted">—</td>`;
    const d = Math.round((cumul - pb) * 10) / 10;
    const cls = d < 0 ? "d-ahead" : d > 0 ? "d-behind" : "muted";
    return `<td class="temps ${cls}">${fmtDeltaSr(d)}</td>`;
  };

  // Meilleur segment de la run → mise en avant dorée (convention LiveSplit).
  // Seuls les segments bouclés comptent ; 0 = durée absente/illisible.
  let bestIdx = -1;
  let bestVal = Infinity;
  sDurees.forEach((d, i) => {
    if (!sCumuls[i]) return;
    const v = parseTempsSr(d);
    if (v > 0 && v < bestVal) {
      bestVal = v;
      bestIdx = i;
    }
  });

  const lignes = segs
    .map((seg, i) => {
      const fait = !!sCumuls[i];
      const courant =
        idx !== null && i === idx && (phase === "Running" || phase === "Paused");
      const skippe = idx !== null && i < idx && !fait;
      const gold = i === bestIdx;
      const pb = pbCol(i);
      let duree = "—";
      let total = pb !== null ? `<span class="muted">${fmtTempsCs(pb)}</span>` : "—";
      let delta = `<td class="temps muted">—</td>`;
      if (fait) {
        duree = sDurees[i] || "—";
        total = sCumuls[i]!;
        delta = celluleDelta(parseTempsSr(sCumuls[i]), pb);
      } else if (courant) {
        duree = segLive ?? "—";
        total = live;
        delta = celluleDelta(parseTempsSr(live), pb);
      }
      const tag = skippe
        ? `<span class="tag tag-skip">${esc(tr("widgetSpeedrun.exportSkip"))}</span>`
        : courant
          ? `<span class="tag tag-live">${esc(tr("widgetSpeedrun.exportTagEnCours"))}</span>`
          : "";
      const cls = courant ? "actif" : gold ? "gold" : "";
      return `<tr${cls ? ` class="${cls}"` : ""}><td class="num">${i + 1}</td><td class="nom">${esc(seg.nom)}${tag}</td><td class="temps${gold ? " d-gold" : ""}">${duree}</td><td class="temps">${total}</td>${delta}</tr>`;
    })
    .join("\n");

  // Temps total d'en-tête : chrono vivant si la run tourne, sinon le dernier
  // cumul réalisé (« — » si aucun split bouclé).
  const dernierCumul = [...sCumuls].reverse().find((c) => !!c) ?? null;
  const totalRun =
    phase === "Running" || phase === "Paused" ? live : (dernierCumul ?? "—");
  const statut =
    phase === "Running"
      ? tr("widgetSpeedrun.exportStatutEnCours")
      : phase === "Paused"
        ? tr("widgetSpeedrun.exportStatutEnPause")
        : phase === "Ended"
          ? tr("widgetSpeedrun.exportStatutTerminee")
          : tr("widgetSpeedrun.exportStatutArretee");
  const dateStr = new Date().toLocaleString(get(localeTag));

  const html = `<!DOCTYPE html>
<html lang="${esc(get(langue))}">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>${esc(jeu || "Splits")} — ${esc(tr("widgetSpeedrun.exportPartie", { n }))}</title>
<style>
  :root { color-scheme: dark; }
  * { margin: 0; padding: 0; box-sizing: border-box; }
  body {
    min-height: 100vh; display: flex; align-items: center; justify-content: center;
    padding: 40px 20px; color: #e8eaf2;
    font-family: "Segoe UI", system-ui, -apple-system, sans-serif;
    background: radial-gradient(1100px 560px at 50% -12%, #1d1733 0%, #0d0f14 58%);
  }
  .carte {
    position: relative; overflow: hidden; width: min(740px, 100%);
    background: #141824; border: 1px solid rgba(255,255,255,.07);
    border-radius: 18px;
    padding: clamp(18px, 4vw, 34px) clamp(16px, 5vw, 38px) clamp(18px, 4vw, 28px);
    box-shadow: 0 24px 60px rgba(0,0,0,.5);
  }
  .carte::before {
    content: ""; position: absolute; inset: 0 0 auto 0; height: 3px;
    background: linear-gradient(90deg, #8b5cf6, #22d3ee);
  }
  .jeu { font-size: 26px; font-weight: 700; letter-spacing: .4px; }
  .cat { margin-top: 4px; font-size: 14px; color: #9aa3b8; }
  .meta { display: flex; flex-wrap: wrap; gap: 10px; margin: 18px 0 4px; }
  .chip {
    font-size: 11.5px; letter-spacing: .8px; text-transform: uppercase;
    padding: 4px 12px; border-radius: 999px; color: #c4cad9;
    border: 1px solid rgba(255,255,255,.12); background: rgba(255,255,255,.04);
  }
  .chip.partie { border-color: rgba(139,92,246,.55); color: #c4b5fd; }
  .chip.statut { border-color: rgba(34,211,238,.5); color: #67e8f9; }
  .total-label {
    margin-top: 20px; font-size: 11px; letter-spacing: 1.6px;
    text-transform: uppercase; color: #7d8598;
  }
  .total {
    font-size: 44px; font-weight: 800; font-variant-numeric: tabular-nums;
    margin: 2px 0 24px; letter-spacing: 1px;
    background: linear-gradient(90deg, #f2f4fa, #9aa3b8);
    -webkit-background-clip: text; background-clip: text; color: transparent;
  }
  .table-wrap {
    overflow: hidden; overflow-x: auto;
    border: 1px solid rgba(255,255,255,.08); border-radius: 12px;
    background: rgba(255,255,255,.02);
  }
  table { width: 100%; border-collapse: collapse; font-size: 14px; }
  thead th {
    text-align: left; padding: 11px 10px; font-size: 10.5px;
    letter-spacing: 1.4px; text-transform: uppercase; color: #9aa3b8;
    border-bottom: 1px solid rgba(255,255,255,.12);
    background: linear-gradient(90deg, rgba(139,92,246,.16), rgba(34,211,238,.09));
  }
  thead th:first-child { text-align: center; }
  tbody td { padding: 10px; border-bottom: 1px solid rgba(255,255,255,.05); }
  tbody tr:last-child td { border-bottom: none; }
  tbody tr:nth-child(even) { background: rgba(255,255,255,.02); }
  tbody tr:hover { background: rgba(255,255,255,.05); }
  tr.actif { background: rgba(139,92,246,.12); }
  tr.actif td:first-child { box-shadow: inset 3px 0 0 #8b5cf6; }
  tr.gold { background: rgba(251,191,36,.05); }
  tr.gold td.nom::before {
    content: ""; display: inline-block; width: 7px; height: 7px;
    margin-right: 9px; border-radius: 50%; background: #fbbf24;
    box-shadow: 0 0 6px rgba(251,191,36,.7); vertical-align: 1px;
  }
  td.num {
    width: 40px; text-align: center; color: #7d8598;
    font-variant-numeric: tabular-nums;
  }
  td.nom { font-weight: 600; }
  .temps {
    text-align: right; white-space: nowrap;
    font-family: "Cascadia Mono", Consolas, monospace;
    font-variant-numeric: tabular-nums;
  }
  .muted { color: #5d6474; }
  .d-ahead { color: #4ade80; }
  .d-behind { color: #f87171; }
  .d-gold { color: #fbbf24; font-weight: 700; }
  .tag {
    display: inline-block; margin-left: 10px; padding: 1px 7px;
    font-size: 10px; font-weight: 700; letter-spacing: 1px;
    text-transform: uppercase; border-radius: 4px; vertical-align: middle;
  }
  .tag-skip { color: #fbbf24; border: 1px solid rgba(251,191,36,.45); }
  .tag-live { color: #67e8f9; border: 1px solid rgba(103,232,249,.45); }
  footer {
    margin-top: 24px; text-align: center; font-size: 11px; color: #5d6474;
  }
  @media (max-width: 560px) {
    body { padding: 18px 10px; }
    .jeu { font-size: 20px; }
    .cat { font-size: 13px; }
    .meta { gap: 8px; }
    .chip { font-size: 10.5px; }
    .total { font-size: 32px; margin-bottom: 18px; }
    thead th { padding: 9px 8px; font-size: 9.5px; letter-spacing: 1px; }
    tbody td { padding: 9px 8px; font-size: 13px; }
    td.num { width: 30px; }
    .tag { margin-left: 6px; }
    footer { font-size: 10px; }
  }
</style>
</head>
<body>
  <main class="carte">
    <div class="jeu">${esc(jeu || "Speedrun")}</div>
    ${cat ? `<div class="cat">${esc(cat)}</div>` : ""}
    <div class="meta">
      <span class="chip partie">${esc(tr("widgetSpeedrun.exportPartie", { n }))}</span>
      <span class="chip statut">${esc(statut)}</span>
    </div>
    <div class="total-label">${esc(tr("widgetSpeedrun.exportTotal"))}</div>
    <div class="total">${esc(totalRun)}</div>
    <div class="table-wrap">
    <table>
      <thead>
        <tr>
          <th>#</th>
          <th>${esc(tr("widgetSpeedrun.exportColSplit"))}</th>
          <th class="temps">${esc(tr("widgetSpeedrun.exportColSegment"))}</th>
          <th class="temps">${esc(tr("widgetSpeedrun.exportColTotal"))}</th>
          <th class="temps">${esc(tr("widgetSpeedrun.exportColDelta"))}</th>
        </tr>
      </thead>
      <tbody>
${lignes}
      </tbody>
    </table>
    </div>
    <footer>${esc(tr("widgetSpeedrun.exportGenereLe", { date: dateStr }))}</footer>
  </main>
</body>
</html>
`;

  const slug =
    (jeu || "run")
      .toLowerCase()
      .normalize("NFD")
      .replace(/[̀-ͯ]/g, "")
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/^-+|-+$/g, "") || "run";
  const d = new Date();
  const pad = (x: number) => String(x).padStart(2, "0");
  const nomFichier = `splits-${slug}-partie-${n}-${d.getFullYear()}${pad(d.getMonth() + 1)}${pad(d.getDate())}-${pad(d.getHours())}${pad(d.getMinutes())}.html`;

  // Anti double-déclenchement : un clic = un dialog. Les appels
  // concurrents (listeners empilés ou clics répétés sur le pop-out, qui
  // reste cliquable pendant le dialog modal) sont ignorés tant qu'un
  // export est en cours.
  if (exportEnCours) return;
  exportEnCours = true;
  try {
    const chemin = await tauri.speedrunExporterSplits(html, nomFichier);
    if (chemin) erreurSpeedrun.set(null);
  } catch (e) {
    erreurSpeedrun.set(String(e));
  } finally {
    exportEnCours = false;
  }
}
