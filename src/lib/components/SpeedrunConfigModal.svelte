<script lang="ts">
  // Modale de configuration ASL — reproduit le workflow LiveSplit :
  // quand un ASL est chargé pour la première fois, s'ouvre automatiquement
  // et propose les options (splits individuels) à valider ou pas.
  //
  // Fonctionnalités :
  //   - Arbre des settings groupés par parent (catégorie)
  //   - Auto-mapping : coche les splits dont le nom correspond à un segment LSS
  //   - Presets : "Tout décocher", "Tout cocher", "Auto depuis LSS"
  //   - Recherche filtrante
  //   - Persistance via majSettingsAslEtPersister
  import Modal from "./Modal.svelte";
  import {
    settingsDetaillesAsl,
    nomsSplitsAsl,
    segmentsLss,
    majSettingsAslEtPersister,
  } from "../stores/speedrun";

  let { onFermer }: { onFermer: () => void } = $props();

  // Copie locale des valeurs (éditable) — initialisée depuis le store.
  // On ne pousse vers le backend qu'au "Valider".
  let valeurs = $state<Record<string, boolean>>({});

  // Recherche
  let recherche = $state("");

  // Initialiser les valeurs depuis le store au montage
  $effect(() => {
    const settings = $settingsDetaillesAsl;
    const noms = $nomsSplitsAsl;
    const segs = $segmentsLss;
    // Si les settings sont vides, rien à faire
    if (settings.length === 0) return;
    // Construire la map initiale
    const map: Record<string, boolean> = {};
    for (const s of settings) {
      map[s.id] = s.value;
    }
    // Auto-mapping : si des segments LSS sont chargés, cocher les settings
    // dont le nom (via D.Names.Split) correspond à un segment LSS.
    if (segs.length > 0 && Object.keys(noms).length > 0) {
      // Construire l'index inverse : nom → code
      const nomVersCode: Record<string, string> = {};
      for (const [code, nom] of Object.entries(noms)) {
        nomVersCode[nom] = code;
      }
      // Pour chaque segment LSS, trouver le code correspondant
      for (const seg of segs) {
        const nomNettoye = nettoyerNomSegment(seg.nom);
        const code = nomVersCode[nomNettoye];
        if (code && code in map) {
          map[code] = true;
        }
      }
    }
    valeurs = map;
  });

  /// Nettoie un nom de segment LSS pour le mapping :
  /// - Strippe le préfixe "-" (indicateur subsplit)
  /// - Strippe les annotations "{...}" (ex: "{Vent Clip}Cell" → "Cell")
  function nettoyerNomSegment(nom: string): string {
    let result = nom;
    // Stripper les annotations {...} au début
    result = result.replace(/^\{[^}]*\}/, "");
    // Stripper le préfixe "-" (subsplit)
    result = result.replace(/^-/, "");
    return result.trim();
  }

  // Settings groupés par parent
  let settingsGroupes = $derived.by(() => {
    const settings = $settingsDetaillesAsl;
    const groupes: { parent: string | null; items: typeof settings }[] = [];
    const parParent = new Map<string | null, typeof settings>();
    for (const s of settings) {
      const key = s.parent;
      if (!parParent.has(key)) parParent.set(key, []);
      parParent.get(key)!.push(s);
    }
    for (const [parent, items] of parParent) {
      groupes.push({ parent, items });
    }
    return groupes;
  });

  // Settings filtrés par recherche
  function settingVisible(label: string, id: string): boolean {
    if (!recherche.trim()) return true;
    const q = recherche.toLowerCase();
    return label.toLowerCase().includes(q) || id.toLowerCase().includes(q);
  }

  // Compter les cochés
  let nbCoche = $derived(
    Object.values(valeurs).filter((v) => v).length
  );
  let nbTotal = $derived($settingsDetaillesAsl.length);

  // Presets
  function toutCocher() {
    const map: Record<string, boolean> = {};
    for (const s of $settingsDetaillesAsl) map[s.id] = true;
    valeurs = map;
  }
  function toutDecocher() {
    const map: Record<string, boolean> = {};
    for (const s of $settingsDetaillesAsl) map[s.id] = false;
    valeurs = map;
  }
  function autoDepuisLss() {
    const noms = $nomsSplitsAsl;
    const segs = $segmentsLss;
    const map: Record<string, boolean> = {};
    // D'abord tout décocher
    for (const s of $settingsDetaillesAsl) map[s.id] = false;
    // Puis cocher les splits correspondant aux segments LSS
    if (segs.length > 0 && Object.keys(noms).length > 0) {
      const nomVersCode: Record<string, string> = {};
      for (const [code, nom] of Object.entries(noms)) {
        nomVersCode[nom] = code;
      }
      for (const seg of segs) {
        const nomNettoye = nettoyerNomSegment(seg.nom);
        const code = nomVersCode[nomNettoye];
        if (code && code in map) {
          map[code] = true;
        }
      }
    }
    valeurs = map;
  }

  // Valider
  let enCours = $state(false);
  async function valider() {
    enCours = true;
    try {
      await majSettingsAslEtPersister(valeurs);
      onFermer();
    } finally {
      enCours = false;
    }
  }
</script>

<Modal
  title="Configuration de l'Auto-Splitter"
  maxWidth="700px"
  hint="Cochez les splits que vous souhaitez activer. Les splits cochés se déclencheront automatiquement quand les conditions du jeu sont remplies. Cliquez sur « Auto depuis LSS » pour cocher automatiquement les splits correspondant à votre fichier de splits."
  onClose={onFermer}
>
  <div class="asl-config">
    <!-- Barre d'outils : presets + recherche -->
    <div class="asl-toolbar">
      <div class="asl-presets">
        <button class="preset-btn" onclick={autoDepuisLss} disabled={$segmentsLss.length === 0}>
          Auto depuis LSS
        </button>
        <button class="preset-btn" onclick={toutCocher}>Tout cocher</button>
        <button class="preset-btn" onclick={toutDecocher}>Tout décocher</button>
      </div>
      <input
        class="asl-recherche"
        type="text"
        placeholder="Rechercher un split…"
        bind:value={recherche}
      />
    </div>

    <!-- Compteur -->
    <div class="asl-compteur">
      {nbCoche} / {nbTotal} splits activés
    </div>

    <!-- Liste des settings groupés par parent -->
    <div class="asl-liste">
      {#each settingsGroupes as groupe (groupe.parent ?? "__sans_parent__")}
        {#if groupe.items.some((s) => settingVisible(s.label, s.id))}
          <div class="asl-groupe">
            {#if groupe.parent}
              <div class="asl-groupe-titre">{groupe.parent}</div>
            {/if}
            {#each groupe.items as s (s.id)}
              {#if settingVisible(s.label, s.id)}
                <label class="asl-setting" title={s.id}>
                  <input
                    type="checkbox"
                    checked={valeurs[s.id] ?? false}
                    onchange={(e) => {
                      valeurs = { ...valeurs, [s.id]: e.currentTarget.checked };
                    }}
                  />
                  <span class="asl-setting-label">{s.label}</span>
                  <span class="asl-setting-code">{s.id}</span>
                </label>
              {/if}
            {/each}
          </div>
        {/if}
      {/each}
    </div>

    <!-- Boutons -->
    <div class="asl-actions">
      <button class="asl-btn annuler" onclick={onFermer} disabled={enCours}>
        Annuler
      </button>
      <button class="asl-btn valider" onclick={valider} disabled={enCours}>
        {enCours ? "Application…" : "Valider"}
      </button>
    </div>
  </div>
</Modal>

<style>
  .asl-config {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }

  .asl-toolbar {
    display: flex;
    gap: 0.5rem;
    padding: 0.6rem 1rem;
    border-bottom: 1px solid var(--bordure);
    align-items: center;
  }
  .asl-presets {
    display: flex;
    gap: 0.3rem;
    flex-shrink: 0;
  }
  .preset-btn {
    background: var(--btn-surface);
    color: var(--texte);
    border: 1px solid var(--bordure);
    border-radius: var(--rayon-petit);
    padding: 0.25rem 0.5rem;
    font-size: 0.78rem;
    cursor: pointer;
    transition: background 0.15s ease, border-color 0.15s ease;
  }
  .preset-btn:hover:not(:disabled) {
    background: var(--btn-surface-hover);
    border-color: var(--accent-violet);
  }
  .preset-btn:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
  .asl-recherche {
    flex: 1;
    background: var(--fond-panneau);
    color: var(--texte);
    border: 1px solid var(--bordure);
    border-radius: var(--rayon-petit);
    padding: 0.25rem 0.5rem;
    font-size: 0.85rem;
  }
  .asl-recherche:focus {
    outline: none;
    border-color: var(--accent-violet);
  }

  .asl-compteur {
    padding: 0.3rem 1rem;
    font-size: 0.78rem;
    color: var(--gris);
    border-bottom: 1px solid var(--bordure);
  }

  .asl-liste {
    flex: 1;
    overflow-y: auto;
    padding: 0.5rem 1rem;
    min-height: 0;
    scrollbar-width: thin;
    scrollbar-color: rgba(255, 255, 255, 0.2) transparent;
  }
  .asl-liste::-webkit-scrollbar { width: 5px; }
  .asl-liste::-webkit-scrollbar-thumb {
    background: rgba(255, 255, 255, 0.2);
    border-radius: 999px;
  }

  .asl-groupe {
    margin-bottom: 0.6rem;
  }
  .asl-groupe-titre {
    font-size: 0.8rem;
    font-weight: 600;
    color: var(--accent-violet);
    padding: 0.3rem 0;
    border-bottom: 1px solid color-mix(in srgb, var(--bordure) 50%, transparent);
    margin-bottom: 0.2rem;
  }

  .asl-setting {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.2rem 0.3rem;
    cursor: pointer;
    border-radius: var(--rayon-petit);
    transition: background 0.1s ease;
  }
  .asl-setting:hover {
    background: color-mix(in srgb, var(--accent-violet) 8%, transparent);
  }
  .asl-setting input[type="checkbox"] {
    flex-shrink: 0;
    accent-color: var(--accent-violet);
  }
  .asl-setting-label {
    flex: 1;
    font-size: 0.85rem;
    color: var(--texte);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .asl-setting-code {
    font-size: 0.7rem;
    color: var(--gris);
    font-family: monospace;
    flex-shrink: 0;
    opacity: 0.6;
  }

  .asl-actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
    padding: 0.6rem 1rem;
    border-top: 1px solid var(--bordure);
  }
  .asl-btn {
    padding: 0.4rem 1rem;
    border-radius: var(--rayon-petit);
    font-size: 0.85rem;
    cursor: pointer;
    border: 1px solid var(--bordure);
    transition: background 0.15s ease, border-color 0.15s ease;
  }
  .asl-btn.annuler {
    background: var(--btn-surface);
    color: var(--texte);
  }
  .asl-btn.annuler:hover:not(:disabled) {
    background: var(--btn-surface-hover);
  }
  .asl-btn.valider {
    background: var(--accent-violet);
    color: white;
    border-color: var(--accent-violet);
    font-weight: 600;
  }
  .asl-btn.valider:hover:not(:disabled) {
    background: color-mix(in srgb, var(--accent-violet) 85%, white);
  }
  .asl-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
</style>
