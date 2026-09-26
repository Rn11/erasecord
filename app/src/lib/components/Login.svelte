<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { api, asCommandError } from "$lib/api";
  import type { User } from "$lib/types";

  let { notice = null, onLogin }: { notice?: string | null; onLogin: (user: User, rememberError: string | null) => void } =
    $props();

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
      error = asCommandError(err).message;
    } finally {
      busy = false;
    }
  }
</script>

<div class="wrap">
  <form class="card login" onsubmit={submit}>
    <header>
      <h1>EraseCord</h1>
      <p class="muted">Delete your own Discord messages from the servers and DMs you choose.</p>
    </header>

    {#if notice}
      <p class="callout info small">{notice}</p>
    {/if}

    <div class="callout warn small risk">
      <strong>Before you start</strong>
      <p>
        EraseCord logs in with your user token. Automating a user account is against Discord's Terms of Service, so
        Discord could limit or ban your account. EraseCord only deletes, sends one request at a time and waits
        whenever Discord asks it to, but the risk does not go away. Deleted messages cannot be restored.
      </p>
      <label class="check">
        <input type="checkbox" bind:checked={acknowledged} />
        <span>I understand the risk and want to continue</span>
      </label>
    </div>

    <label class="field">
      <span>Discord token</span>
      <div class="token-row">
        <input
          type={reveal ? "text" : "password"}
          bind:value={token}
          autocomplete="off"
          spellcheck="false"
          placeholder="Paste your token"
        />
        <button type="button" class="btn ghost small" onclick={() => (reveal = !reveal)}>
          {reveal ? "Hide" : "Show"}
        </button>
      </div>
    </label>

    <label class="check">
      <input type="checkbox" bind:checked={remember} />
      <span>Remember on this device <span class="muted">(stored in the system's credential store)</span></span>
    </label>

    {#if error}
      <p class="callout error small" role="alert">{error}</p>
    {/if}

    <button class="btn primary submit" type="submit" disabled={!canSubmit}>
      {#if busy}<span class="spinner"></span> Checking token…{:else}Log in{/if}
    </button>

    <details class="help small">
      <summary>How do I find my token?</summary>
      <ol>
        <li>
          Open <button type="button" class="link" onclick={() => openUrl("https://discord.com/app")}>discord.com/app</button>
          in your browser and log in.
        </li>
        <li>Open the developer tools (<kbd>F12</kbd>, or <kbd>⌥</kbd><kbd>⌘</kbd><kbd>I</kbd> on macOS) and switch to the <em>Network</em> tab.</li>
        <li>Type <code>api</code> into the filter box, then click on any channel so requests show up.</li>
        <li>Select one of the requests. Under <em>Request Headers</em>, the value of <code>authorization</code> is your token.</li>
      </ol>
      <p class="muted">
        Your token gives full access to your account: never share it. EraseCord sends it only to discord.com.
        Changing your password makes the token invalid.
      </p>
    </details>
  </form>
</div>

<style>
  .wrap {
    min-height: 100%;
    display: grid;
    place-items: center;
    padding: 32px 16px;
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

  .link {
    background: none;
    border: none;
    padding: 0;
    font: inherit;
    color: var(--accent);
    text-decoration: underline;
    cursor: pointer;
  }

  kbd,
  code {
    font-family: ui-monospace, "SF Mono", Menlo, Consolas, monospace;
    font-size: 12px;
    background: var(--panel-2);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 0 4px;
  }
</style>
