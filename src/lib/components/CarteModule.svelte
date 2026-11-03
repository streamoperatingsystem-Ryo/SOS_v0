<script lang="ts">
  // Carte d'édition d'un module HUD — même carte latérale que les widgets
  // (2026-11-02). Remplace la carte flottante du canvas (14 rem, ancrée en
  // haut-gauche) et donne enfin une vraie carte au module Spots.
  //
  // Le détail exhaustif (lignes d'objectifs, couleurs d'équaliseur…) reste
  // dans la modale Modules via « Configurer… » — la carte porte l'essentiel :
  // afficher, position, mise en forme, options rapides.
  import { moduleCarteId, modulesModalOpen, carteEditionCible } from "../stores/ui";
  import { sceneStore, setModule, setModuleActif, moduleDefaut } from "../stores/scene";
  import { POLICES_TITRE } from "../fonts";
  import { t } from "../stores/langue";
  import type { ModuleHud } from "../tauri";

  const ICONES: Record<string, string> = {
    horloge: "🕒",
    musique: "♫",
    objectifs: "🎯",
    equaliseur: "🎚",
    spots: "🔦",
  };

  let id = $derived($moduleCarteId);
  let m = $derived(
    id
      ? ($sceneStore.modules?.[id] ??
          moduleDefaut(id, $sceneStore.canvasW ?? 1920, $sceneStore.canvasH ?? 1080))
      : null,
  );
  let actif = $derived(m?.actif === true);

  // Mutuellement exclusive avec la carte widget/fond.
  $effect(() => {
    if ($carteEditionCible !== null) moduleCarteId.set(null);
  });

  function fermer() {
    moduleCarteId.set(null);
  }
  function patch(p: Partial<ModuleHud>) {
    if (id) void setModule(id, p);
  }
  function num(e: Event) {
    return Math.max(0, Math.round(+(e.target as HTMLInputElement).value || 0));
  }
  function ouvrirAvance() {
    if (!id) return;
    modulesModalOpen.set(id);
    moduleCarteId.set(null);
  }
</script>

{#if id && m}
  <div class="carte-edition module-carte">
    <div class="cm-tete">
      <span class="cm-icone" aria-hidden="true">{ICONES[id]}</span>
      <span class="cm-titre">{$t("modules.nom." + id)}</span>
      <span class="cm-etat" class:on={actif}>{actif ? "ON" : "OFF"}</span>
      <button
        type="button"
        class="cm-fermer"
        onclick={fermer}
        title={$t("commun.fermer")}
        aria-label={$t("commun.fermer")}
      >✕</button>
    </div>

    <div class="cm-corps">
      <label class="cm-toggle">
        <input
          type="checkbox"
          checked={actif}
          onchange={(e) => void setModuleActif(id!, e.currentTarget.checked)}
        />
        <span>{$t("modules.afficher")}</span>
      </label>

      {#if id !== "spots"}
        <div class="cm-section">{$t("modules.position")}</div>
        <div class="cm-pos">
          <label class="cm-field"><span class="field-label">X</span>
            <input type="number" min="0" value={m.x} onchange={(e) => patch({ x: num(e) })} />
          </label>
          <label class="cm-field"><span class="field-label">Y</span>
            <input type="number" min="0" value={m.y} onchange={(e) => patch({ y: num(e) })} />
          </label>
          <label class="cm-field"><span class="field-label">{$t("commun.largeur")}</span>
            <input type="number" min="0" value={m.largeur} onchange={(e) => patch({ largeur: num(e) })} />
          </label>
          <label class="cm-field"><span class="field-label">{$t("commun.hauteur")}</span>
            <input type="number" min="0" value={m.hauteur} onchange={(e) => patch({ hauteur: num(e) })} />
          </label>
        </div>
      {:else}
        <p class="hint">{$t("modules.hint.spots")}</p>
      {/if}

      <div class="cm-section">{$t("modules.style")}</div>
      <div class="cm-pos">
        <label class="cm-field">
          <span class="field-label">{$t("commun.police")}</span>
          <select value={m.police ?? ""} onchange={(e) => patch({ police: (e.target as HTMLSelectElement).value || undefined })}>
            <option value="">{$t("modules.policeDefaut")}</option>
            {#each POLICES_TITRE as p (p.id)}
              <option value={p.id} style="font-family: '{p.id}', sans-serif;">{p.nom}</option>
            {/each}
          </select>
        </label>
        <label class="cm-field">
          <span class="field-label">{$t("modules.taille")}</span>
          <input type="number" min="0" value={m.taille ?? ""} placeholder="auto"
            onchange={(e) => patch({ taille: num(e) || undefined })} />
        </label>
      </div>

      <label class="cm-range">
        <span>{$t("modules.opacite")} — {Math.round((m.opacite ?? 0.45) * 100)} %</span>
        <input type="range" min="0" max="100" step="5"
          value={Math.round((m.opacite ?? 0.45) * 100)}
          onchange={(e) => patch({ opacite: +(e.target as HTMLInputElement).value / 100 })} />
      </label>

      <label class="cm-toggle">
        <input type="checkbox" checked={m.cadre === true} onchange={(e) => patch({ cadre: e.currentTarget.checked })} />
        <span>{$t("modules.cadre")}</span>
      </label>

      {#if id === "horloge"}
        <div class="cm-section">{$t("modules.options")}</div>
        <label class="cm-toggle">
          <input type="checkbox" checked={m.afficherDate !== false} onchange={(e) => patch({ afficherDate: e.currentTarget.checked })} />
          <span>{$t("modules.horloge.date")}</span>
        </label>
        <label class="cm-toggle">
          <input type="checkbox" checked={m.secondes === true} onchange={(e) => patch({ secondes: e.currentTarget.checked })} />
          <span>{$t("modules.horloge.secondes")}</span>
        </label>
        <label class="cm-toggle">
          <input type="checkbox" checked={m.format12h === true} onchange={(e) => patch({ format12h: e.currentTarget.checked })} />
          <span>{$t("modules.horloge.h12")}</span>
        </label>
      {:else if id === "musique"}
        <div class="cm-section">{$t("modules.options")}</div>
        <label class="cm-toggle">
          <input type="checkbox" checked={m.afficherTemps !== false} onchange={(e) => patch({ afficherTemps: e.currentTarget.checked })} />
          <span>{$t("modules.musique.temps")}</span>
        </label>
      {:else if id === "equaliseur"}
        <div class="cm-section">{$t("modules.options")}</div>
        <label class="cm-toggle">
          <input type="checkbox" checked={m.eqMicro !== false} onchange={(e) => patch({ eqMicro: e.currentTarget.checked })} />
          <span>{$t("modules.equaliseur.micro")}</span>
        </label>
        <label class="cm-toggle">
          <input type="checkbox" checked={m.eqBulles !== false} onchange={(e) => patch({ eqBulles: e.currentTarget.checked })} />
          <span>{$t("modules.equaliseur.bulles")}</span>
        </label>
      {:else if id === "spots"}
        <div class="cm-section">{$t("modules.options")}</div>
        <label class="cm-range">
          <span>{$t("modules.spots.vitesse")} — {(m.spotsVitesse ?? 1).toFixed(1)}×</span>
          <input type="range" min="0.2" max="3" step="0.1"
            value={m.spotsVitesse ?? 1}
            onchange={(e) => patch({ spotsVitesse: +(e.target as HTMLInputElement).value })} />
        </label>
        <label class="cm-toggle">
          <input type="checkbox" checked={m.spotsReagir !== false} onchange={(e) => patch({ spotsReagir: e.currentTarget.checked })} />
          <span>{$t("modules.spots.reagir")}</span>
        </label>
      {:else if id === "objectifs"}
        <p class="hint">{$t("modules.objectifs.vide")}</p>
      {/if}

      <button type="button" class="action" onclick={ouvrirAvance}>
        {$t("commun.configurer")}…
      </button>
    </div>
  </div>
{/if}

<style>
  .module-carte {
    --accent-carte: var(--accent-ui);
    --accent-carte-glow: var(--glow-ui);
  }
  .cm-tete {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.45rem 0.35rem 0.45rem 0.55rem;
    border-bottom: 1px solid var(--accent-carte);
    background: linear-gradient(135deg, color-mix(in srgb, var(--accent-carte) 15%, transparent), transparent);
  }
  .cm-icone {
    font-size: 0.85rem;
    line-height: 1;
    flex-shrink: 0;
  }
  .cm-titre {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    font-size: 0.8rem;
    font-weight: 600;
    color: var(--accent-carte);
  }
  .cm-etat {
    font-size: 0.58rem;
    font-weight: 700;
    letter-spacing: 0.06em;
    padding: 0.05rem 0.35rem;
    border: 1px solid var(--bordure-ui);
    border-radius: 999px;
    color: var(--etat-inactif);
    opacity: 0.7;
    flex-shrink: 0;
  }
  .cm-etat.on {
    color: var(--etat-ok);
    border-color: var(--etat-ok);
    opacity: 1;
  }
  .cm-fermer {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 1.125rem;
    height: 1.125rem;
    flex-shrink: 0;
    border: 1px solid color-mix(in srgb, var(--accent-carte) 50%, transparent);
    border-radius: var(--rayon-petit);
    background: var(--btn-surface);
    box-shadow: var(--btn-inset);
    color: var(--accent-carte);
    cursor: pointer;
    font-size: 0.55rem;
    line-height: 1;
  }
  .cm-fermer:hover {
    background: var(--accent-carte);
    color: #fff;
    border-color: var(--accent-carte);
    box-shadow: var(--btn-inset-hover);
  }
  .cm-corps {
    display: flex;
    flex-direction: column;
    gap: 0.45rem;
    padding: 0.55rem 0.6rem;
    max-height: 62vh;
    overflow-y: auto;
    scrollbar-width: thin;
    scrollbar-color: var(--accent-carte-glow) transparent;
  }
  .cm-section {
    font-size: 0.66rem;
    font-weight: 700;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    opacity: 0.6;
    margin-top: 0.15rem;
  }
  .cm-pos {
    display: grid;
    grid-template-columns: repeat(2, 1fr);
    gap: 0.4rem;
  }
  .cm-field {
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
    min-width: 0;
  }
  .cm-field input,
  .cm-field select {
    width: 100%;
    min-width: 0;
    background: var(--fond-controle);
    color: var(--texte);
    border: 1px solid var(--bordure-ui);
    border-radius: var(--rayon-petit);
    padding: 0.25rem 0.4rem;
    font: inherit;
    font-size: 0.8rem;
  }
  .cm-toggle {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    font-size: 0.8rem;
    cursor: pointer;
  }
  .cm-range {
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
    font-size: 0.75rem;
  }
  .cm-range input {
    width: 100%;
  }
</style>
