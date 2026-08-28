<script lang="ts">
  // Modal Device Code Flow Twitch. Affiche user_code + bouton ouvrir lien +
  // bouton Annuler. Se ferme sur twitch:connecte (géré par le store qui clear
  // twitchDevice) ou sur Annuler (annulerDeviceFlow).
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { twitchDevice, annulerDeviceFlow } from "../stores/chat";

  let device = $derived($twitchDevice);

  async function onOuvrirLien() {
    if (!device) return;
    try {
      await openUrl(device.verification_uri);
    } catch (e) {
      console.error("openUrl:", e);
    }
  }

  async function onAnnuler() {
    await annulerDeviceFlow();
  }
</script>

{#if device}
  <div class="overlay" role="dialog" aria-modal="true">
    <div class="modal">
      <h2>Connexion Twitch</h2>
      <p class="hint">
        Ouvre le lien, entre ce code, puis autorise l'accès.
      </p>
      <div class="code-box">
        <span class="code">{device.user_code}</span>
      </div>
      <button class="action" onclick={onOuvrirLien}>Ouvrir le lien</button>
      <button class="action" onclick={onAnnuler}>Annuler</button>
    </div>
  </div>
{/if}

<style>
  .overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.6);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 10000;
  }
  .modal {
    background: var(--fond);
    color: var(--texte);
    border: 1px solid var(--texte);
    padding: 1.2rem;
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
    min-width: 18rem;
    max-width: 24rem;
  }
  h2 {
    font-size: 1rem;
    margin: 0;
  }
  .hint {
    font-size: 0.8rem;
    opacity: 0.7;
    line-height: 1.3;
  }
  .code-box {
    border: 1px solid var(--texte);
    padding: 0.6rem;
    text-align: center;
  }
  .code {
    font-size: 1.6rem;
    font-weight: 600;
    letter-spacing: 0.15em;
    font-variant-numeric: tabular-nums;
  }
  .action {
    background: var(--fond);
    color: var(--texte);
    border: 1px solid var(--texte);
    padding: 0.4rem 0.6rem;
    font: inherit;
    cursor: pointer;
    text-align: center;
  }
  .action:hover {
    background: var(--texte);
    color: var(--fond);
  }
</style>
