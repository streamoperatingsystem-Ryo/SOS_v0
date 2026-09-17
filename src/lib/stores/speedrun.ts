// Store speedrun côté dashboard. Écoute l'event Tauri speedrun:event et expose
// l'état du timer, des splits, de la connexion processus et des erreurs.
// Pattern identique à chat.ts : stores writables + initSpeedrun() au montage.
import { writable } from "svelte/store";
import { listen } from "@tauri-apps/api/event";
import { tauri, type SpeedrunEvent, type SpeedrunConfig, type AslSettingDetail } from "../tauri";
import { speedrunConfigModalOpen } from "./ui";

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

/// Liste des temps de split (String formaté, index = numéro du split).
export const splits = writable<string[]>([]);

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

// ===== Init =====

let unlisten: (() => void) | null = null;

/// Initialise le store : écoute l'event Tauri speedrun:event et met à jour
/// les stores. Idempotent — safe d'appeler plusieurs fois.
export async function initSpeedrun() {
  if (unlisten) return;
  unlisten = await listen<SpeedrunEvent>("speedrun:event", (e) => {
    const event = e.payload;
    switch (event.type) {
      case "started":
        phaseTimer.set("Running");
        estActif.set(true);
        indexSplitCourant.set(0);
        break;
      case "split":
        if (event.index !== undefined) {
          indexSplitCourant.set(event.index);
        }
        if (event.time && event.index !== undefined) {
          // event.index est 1-based après incrément (current_split_index Rust).
          // Stocker à 0-based : segIdx = event.index - 1.
          const segIdx = event.index - 1;
          splits.update((s) => {
            while (s.length <= segIdx) s.push("");
            s[segIdx] = event.time!;
            return s;
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
        }
        break;
      case "reset":
        phaseTimer.set("NotRunning");
        indexSplitCourant.set(null);
        splits.set([]);
        tempsJeu.set(null);
        tempsReel.set("00:00.00");
        break;
      case "ended":
        phaseTimer.set("Ended");
        break;
      case "gameTime":
        if (event.time) tempsJeu.set(event.time);
        break;
      case "time":
        if (event.realTime) tempsReel.set(event.realTime);
        if (event.gameTime !== undefined) tempsJeu.set(event.gameTime);
        else if (event.realTime) tempsJeu.set(event.realTime);
        break;
      case "connected":
        estConnecte.set(true);
        if (event.process) nomProcessus.set(event.process);
        break;
      case "disconnected":
        estConnecte.set(false);
        nomProcessus.set(null);
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
    }
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
    return config;
  } catch {
    return null;
  }
}

/// Sauve les préférences speedrun (chemins ASL/LSS + settings).
export async function sauverPreferencesSpeedrun(
  cheminAslVal: string | null,
  cheminLssVal: string | null,
  settings: { start: boolean; split: boolean; reset: boolean }
) {
  try {
    await tauri.speedrunSauverConfig({
      chemin_asl: cheminAslVal ?? undefined,
      chemin_lss: cheminLssVal ?? undefined,
      settings,
    });
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
