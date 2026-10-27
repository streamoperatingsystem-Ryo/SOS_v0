// Métronome du splitter speedrun — battement BPM joué sur le DASHBOARD
// uniquement (Web Audio), jamais envoyé à la diffusion. Suit la phase du
// chrono via phaseTimer (speedrun.ts) : start ASL/manuel → il joue ;
// pause/reset/fin/déconnexion → il s'arrête ; reprise → il repart.
//
// Config = celle du widget speedrun (champs srMetronomeActif/Bpm/Volume,
// persistés dans la scène au même titre que les chemins ASL/LSS). Avec
// plusieurs widgets speedrun, le DERNIER qui pousse sa config gagne —
// même règle de facto que les chemins ASL/LSS (moteur unique, un seul
// réglage actif à la fois).
//
// Précision : les tics sont programmés sur l'horloge AudioContext.currentTime
// (scheduler look-ahead 25 ms → ~120 ms d'avance) → DÉRIVE NULLE, pas un
// setInterval + new Audio() qui se décale de ~100 ms par tic.
import { writable, get } from "svelte/store";
import { phaseTimer } from "./speedrun";

// ===== Stores exposés =====

/// True pendant que le battement réel joue (run en cours + métronome
/// actif). La carte speedrun désactive le bouton « Tester » pendant une run.
export const battementEnCours = writable(false);

// ===== Config courante (poussée par les widgets speedrun) =====

let cfgActif = false;
let cfgBpm = 120;
let cfgVolume = 0.5;

const BPM_MIN = 1;
const BPM_MAX = 400;

// ===== Runtime Web Audio =====

let ctx: AudioContext | null = null;
let schedulerId: number | null = null;
let prochainePulsation = 0;

/// Programme UN tic à l'instant `when` (horloge AudioContext) : sinus
/// 1250 Hz, attaque 1 ms, décroissance exponentielle ~90 ms. Volume lu au
/// moment de la programmation (~120 ms d'avance max).
function jouerPulsation(when: number): void {
  if (!ctx) return;
  const osc = ctx.createOscillator();
  const gain = ctx.createGain();
  osc.frequency.value = 1250;
  gain.gain.setValueAtTime(0, when);
  gain.gain.linearRampToValueAtTime(cfgVolume, when + 0.001);
  gain.gain.exponentialRampToValueAtTime(0.0001, when + 0.09);
  osc.connect(gain);
  gain.connect(ctx.destination);
  osc.start(when);
  osc.stop(when + 0.1);
}

/// Boucle scheduler (25 ms) : programme les tics jusqu'à ~120 ms à l'avance.
/// Relit cfgBpm/cfgVolume à chaque passage → un changement de réglage en
/// cours de run s'applique SANS réinitialiser la phase du battement.
function boucleScheduler(): void {
  if (!ctx) return;
  // Politique autoplay : si le contexte est suspendu (aucun geste encore),
  // retenter — le premier clic de l'utilisateur le débloque.
  if (ctx.state === "suspended") void ctx.resume();
  const periode = 60 / cfgBpm;
  while (prochainePulsation < ctx.currentTime + 0.12) {
    jouerPulsation(prochainePulsation);
    prochainePulsation += periode;
  }
}

/// Démarre le battement (contexte neuf + scheduler). Stoppe d'abord
/// l'éventuel battement précédent (reprise de pause = repart du 1er tic).
function demarrerBattement(): void {
  arreterBattement();
  if (!cfgActif || cfgBpm < BPM_MIN) return;
  ctx = new AudioContext();
  if (ctx.state === "suspended") void ctx.resume();
  prochainePulsation = ctx.currentTime + 0.05;
  boucleScheduler();
  schedulerId = window.setInterval(boucleScheduler, 25);
  battementEnCours.set(true);
}

/// Stoppe le battement (scheduler + contexte fermé).
function arreterBattement(): void {
  if (schedulerId !== null) {
    clearInterval(schedulerId);
    schedulerId = null;
  }
  if (ctx) {
    void ctx.close();
    ctx = null;
  }
  battementEnCours.set(false);
}

// ===== API publique =====

/// Pousse la config métronome d'un widget (champs srMetronome*). Appelé au
/// montage du widget et à chaque changement de ses champs. Application en
/// live : coupure immédiate si actif=false ; démarrage immédiat si une run
/// tourne déjà (phase Running) ; sinon le battement partira au prochain
/// passage en Running.
export function configurerMetronome(actif: boolean, bpm: number, volume: number): void {
  const jouait = get(battementEnCours);
  const actifAvant = cfgActif;
  cfgActif = actif;
  cfgBpm = Math.max(BPM_MIN, Math.min(BPM_MAX, Math.round(bpm || 0)));
  cfgVolume = Math.max(0, Math.min(1, Number.isFinite(volume) ? volume : 0.5));
  if (!cfgActif) {
    arreterBattement();
    return;
  }
  if (get(phaseTimer) === "Running" && (!jouait || !actifAvant)) {
    demarrerBattement();
  }
}

/// Prévisualisation : 4 tics au BPM/volume donnés, sans attendre une run.
/// Sans effet si le battement réel joue (run en cours) — on ne parasite
/// pas le métronome d'une run.
export function testerMetronome(bpm: number, volume: number): void {
  if (get(battementEnCours)) return;
  const b = Math.max(BPM_MIN, Math.min(BPM_MAX, Math.round(bpm || 0)));
  const v = Math.max(0, Math.min(1, Number.isFinite(volume) ? volume : 0.5));
  const testCtx = new AudioContext();
  if (testCtx.state === "suspended") void testCtx.resume();
  const t0 = testCtx.currentTime + 0.05;
  for (let k = 0; k < 4; k++) {
    const osc = testCtx.createOscillator();
    const gain = testCtx.createGain();
    osc.frequency.value = 1250;
    const when = t0 + (k * 60) / b;
    gain.gain.setValueAtTime(0, when);
    gain.gain.linearRampToValueAtTime(v, when + 0.001);
    gain.gain.exponentialRampToValueAtTime(0.0001, when + 0.09);
    osc.connect(gain);
    gain.connect(testCtx.destination);
    osc.start(when);
    osc.stop(when + 0.1);
  }
  // Fermer le contexte après le dernier tic (+ marge).
  window.setTimeout(() => {
    void testCtx.close();
  }, 240000 / b + 500);
}

// ===== Suivi de la phase du chrono =====
// Le store phaseTimer centralise TOUTES les transitions (started/paused/
// resumed/reset/ended/disconnected/stopped/goto + resync etat) → une seule
// souscription suffit. Running → battement ; tout le reste → arrêt. La
// pause stoppe net (reprise = battement repart du premier tic — un
// métronome n'a pas besoin de continuité de phase à travers une pause).
phaseTimer.subscribe((phase) => {
  if (phase === "Running" && cfgActif) demarrerBattement();
  else arreterBattement();
});
