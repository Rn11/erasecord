<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { api } from "$lib/api";
  import { errorMessage } from "$lib/errors";
  import { t } from "$lib/i18n.svelte";
  import LanguagePicker from "./LanguagePicker.svelte";
  import type { User } from "$lib/types";

  let {
    notice = null,
    onLogin,
    onInsights,
  }: {
    notice?: string | null;
    onLogin: (user: User, rememberError: string | null) => void;
    /** Opens Insights without logging in. */
    onInsights?: () => void;
  } = $props();

  let token = $state("");
  let remember = $state(false);
  let acknowledged = $state(false);
  let reveal = $state(false);
  let busy = $state(false);
  let error = $state<string | null>(null);

  const canSubmit = $derived(token.trim().length > 0 && acknowledged && !busy);

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (!canSubmit) return;
    busy = true;
    error = null;
    try {
      const result = await api.login(token, remember);
      token = "";
      onLogin(result.user, result.remember_error);
    } catch (err) {
      error = errorMessage(err);
    } finally {
      busy = false;
    }
  }
</script>

<div class="wrap">
  <form class="card login" onsubmit={submit}>
    <header>
      <div class="title-row">
        <h1>EraseCord</h1>
        <LanguagePicker />
      </div>
      <p class="muted">{t("login.tagline")}</p>
    </header>

    {#if notice}
      <p class="callout info small">{notice}</p>
    {/if}

    <div class="callout warn small risk">
      <strong>{t("login.riskTitle")}</strong>
      <p>{t("login.riskText")}</p>
      <label class="check">
        <input type="checkbox" bind:checked={acknowledged} />
        <span>{t("login.acknowledge")}</span>
      </label>
    </div>

    <label class="field">
      <span>{t("login.token")}</span>
      <div class="token-row">
        <input
          type={reveal ? "text" : "password"}
          bind:value={token}
          autocomplete="off"
          spellcheck="false"
          placeholder={t("login.tokenPlaceholder")}
        />
        <button type="button" class="btn ghost small" onclick={() => (reveal = !reveal)}>
          {reveal ? t("login.hide") : t("login.show")}
        </button>
      </div>
    </label>

    <label class="check">
      <input type="checkbox" bind:checked={remember} />
      <span>{t("login.remember")} <span class="muted">{t("login.rememberHint")}</span></span>
    </label>

    {#if error}
      <p class="callout error small" role="alert">{error}</p>
    {/if}

    <button class="btn primary submit" type="submit" disabled={!canSubmit}>
      {#if busy}<span class="spinner"></span> {t("login.checking")}{:else}{t("login.submit")}{/if}
    </button>

    <details class="help small">
      <summary>{t("login.helpTitle")}</summary>
      <ol>
        <li>{t("login.help1")}</li>
        <li>{t("login.help2")}</li>
        <li>{t("login.help3")}</li>
        <li>{t("login.help4")}</li>
      </ol>
      <button type="button" class="btn small" onclick={() => openUrl("https://discord.com/app")}>{t("login.openDiscord")}</button>
      <p class="muted">{t("login.helpNote")}</p>
    </details>

    {#if onInsights}
      <div class="insights-link small">
        <span class="muted">{t("login.orInsights")}</span>
        <button type="button" class="btn small" onclick={onInsights}>{t("login.insights")}</button>
      </div>
    {/if}
  </form>
</div>

<style>
  .wrap {
    min-height: 100%;
    display: grid;
    place-items: center;
    padding: 32px 16px;
  }

  .insights-link {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    flex-wrap: wrap;
    padding-top: 14px;
    border-top: 1px solid var(--border);
  }

  .login {
    width: min(480px, 100%);
    padding: 28px;
    display: grid;
    gap: 16px;
  }

  header {
    display: grid;
    gap: 4px;
  }

  h1 {
    font-size: 24px;
    letter-spacing: -0.01em;
  }

  .title-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    flex-wrap: wrap;
  }

  .help .btn {
    margin: 4px 0 8px;
  }

  .risk {
    display: grid;
    gap: 8px;
  }

  .field {
    display: grid;
    gap: 6px;
    font-weight: 550;
  }

  .token-row {
    display: flex;
    gap: 6px;
  }

  .token-row input {
    flex: 1;
    font-family: ui-monospace, "SF Mono", Menlo, Consolas, monospace;
    font-weight: 400;
  }

  .check {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    cursor: pointer;
  }

  .check input {
    margin-top: 2px;
  }

  .submit {
    padding: 10px 14px;
  }

  .help summary {
    cursor: pointer;
    color: var(--muted);
  }

  .help ol {
    padding-left: 20px;
    display: grid;
    gap: 6px;
  }
</style>
