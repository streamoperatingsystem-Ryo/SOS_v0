// Store scènes côté dashboard (v0.14).
// Index léger (ids + noms) + scène courante. Une seule scène en RAM
// (sceneStore dans scene.ts). Pas de préchargement des autres scènes.
import { writable, get } from "svelte/store";
import { tauri, type SceneIndex, type SceneResume } from "../tauri";
import { loadScene, selectWidget, syncCapturesOBS } from "./scene";
import { t } from "./langue";

export const scenesIndexStore = writable<SceneIndex[]>([]);
export const currentSceneIdStore = writable<string>("");
export const currentSceneNomStore = writable<string>("");
/// Résumés de toutes les scènes (nb widgets, types, modules, mini-rects)
/// indexés par id — alimente le tableau enrichi « Vos scènes » (mini-map
/// procédurale). Rechargé à chaque ouverture du tableau.
export const scenesResumeStore = writable<Record<string, SceneResume>>({});

/// Charge l'index des scènes depuis Rust (ids + noms seulement).
export async function loadScenesIndex(): Promise<void> {
  try {
    const idx = await tauri.scenesLister();
    scenesIndexStore.set(idx);
  } catch (e) {
    console.error("loadScenesIndex:", e);
  }
}

/// Recharge les résumés de toutes les scènes depuis le disque (petits
/// fichiers JSON — coût négligeable). Appelé à l'ouverture de « Vos scènes ».
export async function loadScenesResume(): Promise<void> {
  try {
    const list = await tauri.scenesResumer();
    scenesResumeStore.set(Object.fromEntries(list.map((r) => [r.id, r])));
  } catch (e) {
    console.error("loadScenesResume:", e);
  }
}

/// Charge la scène courante (id + nom) depuis Rust.
export async function loadCurrentScene(): Promise<void> {
  try {
    const entry = await tauri.sceneCourante();
    currentSceneIdStore.set(entry.id);
    currentSceneNomStore.set(entry.nom);
  } catch (e) {
    console.error("loadCurrentScene:", e);
  }
}

/// Crée une nouvelle scène vide, bascule dessus, recharge l'UI.
export async function createScene(nom: string): Promise<void> {
  try {
    await tauri.sceneCreer(nom);
    await loadScenesIndex();
    await loadCurrentScene();
    await loadScene();
    selectWidget(null);
  } catch (e) {
    alert(get(t)("scenes.creationRefusee", { erreur: String(e) }));
  }
}

/// Ouvre une scène (id), recharge l'UI. Désélectionne le widget courant.
/// Après chargement : sync captures OBS (enable/disable + transform).
export async function openScene(id: string): Promise<void> {
  try {
    await tauri.sceneOuvrir(id);
    await loadScenesIndex();
    await loadCurrentScene();
    await loadScene();
    selectWidget(null);
    // Sync captures OBS : activer les SOS-Trou-* de cette scène, cacher les
    // autres + masque caméra à la forme du cadre de cette scène.
    await syncCapturesOBS();
  } catch (e) {
    alert(get(t)("scenes.ouvertureRefusee", { erreur: String(e) }));
  }
}

/// Renomme une scène dans l'index. Met à jour le store local.
export async function renameScene(id: string, nom: string): Promise<void> {
  try {
    await tauri.sceneRenommer(id, nom);
    await loadScenesIndex();
    currentSceneNomStore.update((cur) => (cur ? nom : cur));
  } catch (e) {
    alert(get(t)("scenes.renommageRefuse", { erreur: String(e) }));
  }
}

/// Persiste le déplacement d'une scène (glisser-déposer de la barre
/// « Vos scènes »). Le store a DÉJÀ été réordonné de façon optimiste par le
/// composant — en cas d'erreur on recharge l'index pour resynchroniser.
export async function deplacerScene(id: string, position: number): Promise<void> {
  try {
    await tauri.sceneDeplacer(id, position);
  } catch (e) {
    alert(get(t)("scenes.reordonnancementRefuse", { erreur: String(e) }));
    await loadScenesIndex();
  }
}

/// Masque/affiche le titre d'une scène dans sa pastille (œil — l'onglet
/// devient orange quand masqué). Met à jour le store local.
export async function masquerNomScene(id: string, masque: boolean): Promise<void> {
  try {
    await tauri.sceneMasquerNom(id, masque);
    scenesIndexStore.update((idx) =>
      idx.map((s) => (s.id === id ? { ...s, nomMasque: masque } : s))
    );
  } catch (e) {
    alert(get(t)("scenes.masquageRefuse", { erreur: String(e) }));
  }
}

/// Supprime une scène (fichier + entrée index). Si la scène supprimée est la
/// courante, Rust bascule sur la 1ère restante → on recharge la scène RAM.
export async function deleteScene(id: string): Promise<void> {
  try {
    const wasCurrent = id === get(currentSceneIdStore);
    await tauri.sceneSupprimer(id);
    await loadScenesIndex();
    if (wasCurrent) {
      await loadCurrentScene();
      await loadScene();
      selectWidget(null);
    }
  } catch (e) {
    alert(get(t)("scenes.suppressionRefusee", { erreur: String(e) }));
  }
}

/// Duplique une scène sous un nouveau nom, bascule sur la copie (fin de
/// liste). Même chaîne de rechargement que openScene : la copie peut avoir
/// un widget caméra / des trous liés → sync captures OBS après bascule.
export async function duplicateScene(id: string, nom: string): Promise<void> {
  try {
    await tauri.sceneDupliquer(id, nom);
    await loadScenesIndex();
    await loadCurrentScene();
    await loadScene();
    selectWidget(null);
    // Sync captures + masque caméra (la copie peut avoir un widget caméra).
    await syncCapturesOBS();
  } catch (e) {
    alert(get(t)("scenes.duplicationRefusee", { erreur: String(e) }));
  }
}

/// Exporte la scène courante comme pack dossier portable.
/// Demande le nom du pack (input, défaut = nom de scène courante).
/// Dialog = dossier PARENT. Crée <parent>/<nom-pack>/scene.json + medias/.
export async function exportScene(): Promise<void> {
  const defaut = get(currentSceneNomStore) || "scene";
  const nomPack = prompt(get(t)("scenes.nomPackPrompt"), defaut);
  if (nomPack === null) return; // dialog annulé
  if (nomPack.trim().length === 0) {
    alert(get(t)("scenes.nomPackVide"));
    return;
  }
  try {
    await tauri.sceneExporter(nomPack.trim());
  } catch (e) {
    alert(get(t)("scenes.exportRefuse", { erreur: String(e) }));
  }
}

/// Exporte TOUTES les scènes + TOUS les médias dans un seul .zip (dialog
/// « Enregistrer sous » — dossier + nom choisis, un .zip existant est
/// écrasé). Le zip contient le miroir scenes/+medias/ ET un pack par scène.
export async function exportToutZip(): Promise<void> {
  try {
    const chemin = await tauri.scenesExporterZip();
    if (chemin) {
      alert(get(t)("scenes.exportZipOk", { chemin }));
    }
  } catch (e) {
    alert(get(t)("scenes.exportRefuse", { erreur: String(e) }));
  }
}

/// Importe une scène depuis un .json (dialog Ouvrir). Si importé, recharge l'UI.
export async function importScene(): Promise<void> {
  try {
    const entry = await tauri.sceneImporter();
    if (entry) {
      await loadScenesIndex();
      await loadCurrentScene();
      await loadScene();
      selectWidget(null);
    }
  } catch (e) {
    alert(get(t)("commun.importRefuse", { erreur: String(e) }));
  }
}

/// Importe une sauvegarde complète depuis un .zip (dialog « Ouvrir »).
/// Phase 1 (Rust) : analyse de l'archive sans rien écrire → scènes +
/// conflits (ids déjà présents localement). Chaque conflit est tranché
/// ici par un confirm() : OK = remplacer la scène locale, Annuler = la
/// garder. Phase 2 (Rust) : application + restauration des médias.
/// Les scènes locales absentes du zip ne sont jamais touchées.
export async function importToutZip(): Promise<void> {
  try {
    const analyse = await tauri.scenesImporterZip();
    if (!analyse) return; // dialog annulé
    const remplacer: string[] = [];
    for (const c of analyse.conflits) {
      const ok = confirm(
        get(t)("scenes.importZipConflit", {
          nom: c.nomArchive,
          nomLocal: c.nomLocal,
        })
      );
      if (ok) remplacer.push(c.id);
    }
    const bilan = await tauri.scenesImporterZipAppliquer(
      analyse.zipPath,
      remplacer
    );
    // Reload complet : nouvelles scènes dans l'index, scène courante
    // rechargée depuis le disque côté Rust (elle a pu être remplacée).
    await loadScenesIndex();
    await loadCurrentScene();
    await loadScene();
    selectWidget(null);
    // Sync captures OBS : la scène rechargée peut avoir des trous/widgets
    // caméra différents de l'ancienne version.
    await syncCapturesOBS();
    alert(
      get(t)("scenes.importZipOk", {
        scenes: bilan.scenesImportees,
        medias: bilan.mediasRestaures,
        sautees: bilan.scenesSautees,
      })
    );
  } catch (e) {
    alert(get(t)("commun.importRefuse", { erreur: String(e) }));
  }
}
