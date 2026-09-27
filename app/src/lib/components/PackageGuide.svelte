<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { t } from "$lib/i18n.svelte";

  let { open = false }: { open?: boolean } = $props();

  const REQUEST_HELP = "https://support.discord.com/hc/en-us/articles/360004027692-Requesting-a-Copy-of-your-Data";
  const PACKAGE_HELP = "https://support.discord.com/hc/en-us/articles/360004957991-Your-Discord-Data-Package";

  function visit(url: string) {
    openUrl(url).catch(() => window.open(url, "_blank", "noopener"));
  }
</script>

<details class="guide small" {open}>
  <summary>{t("guide.title")}</summary>
  <ol>
    <li>{t("guide.step1")}</li>
    <li>
      {t("guide.step2")}
      <ul>
        <li><strong>{t("guide.messages")}</strong> – {t("guide.messagesWhy")}</li>
        <li><strong>{t("guide.serversAccount")}</strong> – {t("guide.serversAccountWhy")}</li>
        <li><strong>{t("guide.activity")}</strong> – {t("guide.activityWhy")}</li>
      </ul>
    </li>
    <li>{t("guide.step3")}</li>
    <li>{t("guide.step4")}</li>
  </ol>
  <p class="muted">{t("guide.tips")}</p>
  <div class="links">
    <button type="button" class="btn small" onclick={() => visit("https://discord.com/app")}>{t("login.openDiscord")}</button>
    <button type="button" class="btn small" onclick={() => visit(REQUEST_HELP)}>{t("guide.helpRequest")}</button>
    <button type="button" class="btn small" onclick={() => visit(PACKAGE_HELP)}>{t("guide.helpPackage")}</button>
  </div>
</details>

<style>
  .guide {
    text-align: left;
    border: 1px solid var(--border);
    border-radius: 10px;
    padding: 10px 14px;
    width: 100%;
  }

  summary {
    cursor: pointer;
    font-weight: 600;
  }

  ol {
    margin: 10px 0 8px;
    padding-left: 20px;
    display: grid;
    gap: 6px;
  }

  ul {
    margin: 4px 0 0;
    padding-left: 18px;
    display: grid;
    gap: 2px;
  }

  p {
    margin: 0 0 10px;
  }

  .links {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
</style>
