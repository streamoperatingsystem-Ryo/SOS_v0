<script lang="ts">
  // Vu-mètre stéréo accessible — recette .vu-* d'app.css.
  //
  // Deux pistes (G/D) sur une échelle −60…0 dBFS, remplissage dégradé
  // vert → ambre → rouge, repère de peak-hold. La valeur numérique reste
  // visible : la couleur n'est jamais la seule information. Le rôle
  // `meter` porte la sémantique (valeur en dBFS), les barres sont
  // décoratives pour les lecteurs d'écran.
  import { localeTag, t } from "../stores/langue";
  import type { NiveauAudio } from "../tauri";

  let {
    niveau,
    label,
  }: {
    niveau: NiveauAudio | null;
    label: string;
  } = $props();

  const SILENCE = { peak_db: -96, rms_db: -96, hold_db: -96 };

  let gauche = $derived(niveau?.gauche ?? SILENCE);
  let droite = $derived(niveau?.droite ?? SILENCE);
  let peak = $derived(Math.max(gauche.peak_db, droite.peak_db));
  let ecretage = $derived(niveau?.ecretage ?? false);

  let formateur = $derived(
    new Intl.NumberFormat($localeTag, {
      minimumFractionDigits: 1,
      maximumFractionDigits: 1,
    })
  );

  /// dBFS → position 0…100 sur l'échelle −60…0.
  function pct(db: number): number {
    return Math.max(0, Math.min(100, ((db + 60) / 60) * 100));
  }

  function fmt(db: number): string {
    return db <= -95.9 ? "−∞" : formateur.format(db);
  }

  let valeur = $derived(
    peak <= -95.9 ? "−∞ dBFS" : `${formateur.format(peak)} dBFS`
  );
</script>

<div class="vu-bloc">
  <div
    class="vu"
    role="meter"
    aria-label={label}
    aria-valuemin={-60}
    aria-valuemax={0}
    aria-valuenow={Math.max(-60, Math.min(0, Math.round(peak * 10) / 10))}
    aria-valuetext={ecretage ? `${valeur} — ${$t("audio.ecretage")}` : valeur}
  >
    <div class="vu-piste" aria-hidden="true">
      <div class="vu-fond"></div>
      <div class="vu-masque" style="left:{pct(gauche.peak_db)}%"></div>
      <div class="vu-pic" style="left:{pct(gauche.hold_db)}%"></div>
    </div>
    <div class="vu-piste" aria-hidden="true">
      <div class="vu-fond"></div>
      <div class="vu-masque" style="left:{pct(droite.peak_db)}%"></div>
      <div class="vu-pic" style="left:{pct(droite.hold_db)}%"></div>
    </div>
    <span class="vu-valeur" aria-hidden="true">{fmt(peak)}</span>
  </div>
  {#if ecretage}
    <span class="vu-ecretage" role="status">{$t("audio.ecretage")}</span>
  {/if}
</div>
