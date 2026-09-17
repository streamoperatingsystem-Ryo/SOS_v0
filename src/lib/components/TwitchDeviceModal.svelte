<script lang="ts">
  // Modal Device Code Flow (Twitch OU YouTube). Affiche user_code + bouton ouvrir lien +
  // bouton Annuler. Se ferme sur twitch:connecte/youtube:connecte (géré par le store qui clear
  // twitchDevice/youtubeDevice) ou sur Annuler (annulerDeviceFlow/annulerYoutubeDeviceFlow).
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { twitchDevice, annulerDeviceFlow } from "../stores/chat";
  import { youtubeDevice } from "../stores/chat";
  import { annulerYoutubeDeviceFlow } from "../stores/youtube";
  import Modal from "./Modal.svelte";

  let device = $derived($twitchDevice);
  let ytDevice = $derived($youtubeDevice);

  async function onOuvrirLien() {
    const uri = device?.verification_uri || ytDevice?.verification_uri;
    if (!uri) return;
    try {
      await openUrl(uri);
    } catch (e) {
      console.error("openUrl:", e);
    }
  }

  async function onAnnuler() {
    if (device) {
      await annulerDeviceFlow();
    } else if (ytDevice) {
      await annulerYoutubeDeviceFlow();
    }
  }
</script>

{#if device || ytDevice}
  <Modal
    title="Connexion {device ? "Twitch" : "YouTube"}"
    onClose={onAnnuler}
    maxWidth="24rem"
    hint="Ouvre le lien, entre ce code, puis autorise l'accès."
  >
    <div class="device-body">
      <div class="code-box">
        <span class="code">{device?.user_code || ytDevice?.user_code}</span>
      </div>
      <button class="modal-action" onclick={onOuvrirLien}>Ouvrir le lien</button>
      <button class="modal-action" onclick={onAnnuler}>Annuler</button>
    </div>
  </Modal>
{/if}

<style>
  .device-body {
    padding: 1rem;
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
  }
  .code-box {
    border: 1px solid var(--bordure);
    padding: 0.6rem;
    text-align: center;
  }
  .code {
    font-size: 1.6rem;
    font-weight: 600;
    letter-spacing: 0.15em;
    font-variant-numeric: tabular-nums;
  }
</style>
