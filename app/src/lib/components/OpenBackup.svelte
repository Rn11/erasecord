<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { revealItemInDir } from "@tauri-apps/plugin-opener";
  import { api } from "$lib/api";
  import { errorMessage } from "$lib/errors";
  import { num, t } from "$lib/i18n.svelte";
  import type { Opened } from "$lib/types";

  let { onClose }: { onClose: () => void } = $props();

  let source = $state<string | null>(null);
  let passphrase = $state("");
  let passFile = $state<string | null>(null);
  let into = $state<string | null>(null);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let result = $state<Opened | null>(null);

  async function pick(kind: "file" | "folder" | "pass" | "into") {
    try {
      const picked = await open({
        multiple: false,
        directory: kind === "folder" || kind === "into",
        title:
          kind === "pass" ? t("pass.pickFile") : kind === "into" ? t("open.pickInto") : t("open.pickBackup"),
        filters: kind === "file" ? [{ name: t("open.encrypted"), extensions: ["age"] }] : undefined,
      });
      if (typeof picked !== "string") return;
      if (kind === "pass") passFile = picked;
      else if (kind === "into") into = picked;
      else source = picked;
      result = null;
    } catch (err) {
      error = errorMessage(err);
    }
  }

  async function unpack(event: SubmitEvent) {
    event.preventDefault();
    if (!source || !into) return;
    busy = true;
    error = null;
    try {
      result = await api.openBackup(source, passFile && !passphrase ? { file: passFile } : { text: passphrase }, into);
      passphrase = "";
    } catch (err) {
      error = errorMessage(err);
    } finally {
      busy = false;
    }
  }
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && !busy && onClose()} />

<div class="backdrop" role="presentation" onclick={(e) => e.target === e.currentTarget && !busy && onClose()}>
  <div class="card dialog" role="dialog" aria-modal="true" aria-labelledby="open-title">
  <form onsubmit={unpack}>
    <h2 id="open-title">{t("open.title")}</h2>
    <p class="muted small">{t("open.intro")}</p>

    <div class="step">
      <span class="label">{t("open.which")}</span>
      <div class="row">
        <button type="button" class="btn small" onclick={() => pick("file")}>{t("open.file")}</button>
        <button type="button" class="btn small ghost" onclick={() => pick("folder")}>{t("open.folder")}</button>
      </div>
      {#if source}<code class="path" title={source}>{source}</code>{/if}
    </div>

    <label class="step">
      <span class="label">{t("pass.label")}</span>
      <input type="password" autocomplete="off" bind:value={passphrase} placeholder={passFile ? t("open.usingFile") : ""} />
    </label>
    <div class="row small">
      <button type="button" class="link" onclick={() => pick("pass")}>{t("open.passFile")}</button>
      {#if passFile}<code class="path" title={passFile}>{passFile}</code>{/if}
    </div>

    <div class="step">
      <span class="label">{t("open.into")}</span>
      <div class="row">
        <button type="button" class="btn small" onclick={() => pick("into")}>{t("open.pickInto")}</button>
        {#if into}<code class="path" title={into}>{into}</code>{/if}
      </div>
    </div>

    {#if error}<p class="callout error small">{error}</p>{/if}
    {#if result}
      <p class="callout info small">
        {t("open.done", { files: num(result.files), messages: num(result.messages), path: result.folder })}
        <button type="button" class="link" onclick={() => result && revealItemInDir(result.folder)}>{t("open.show")}</button>
      </p>
    {/if}

    <div class="actions">
      <button type="button" class="btn ghost" onclick={onClose} disabled={busy}>{t("common.close")}</button>
      <button type="submit" class="btn primary" disabled={busy || !source || !into || (!passphrase && !passFile)}>
        {#if busy}<span class="spinner"></span> {t("open.opening")}{:else}{t("open.unpack")}{/if}
      </button>
    </div>
  </form>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 60;
    display: grid;
    place-items: center;
    padding: 16px;
    background: rgb(0 0 0 / 0.35);
  }

  .dialog {
    width: min(520px, 100%);
    padding: 22px;
    box-shadow: var(--shadow);
  }

  form {
    display: grid;
    gap: 12px;
  }

  h2,
  p {
    margin: 0;
  }

  .step {
    display: grid;
    gap: 5px;
  }

  .label {
    font-weight: 600;
    font-size: 13px;
  }

  .row {
    display: flex;
    gap: 8px;
    align-items: center;
    flex-wrap: wrap;
  }

  .path {
    font-size: 12px;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .link {
    background: none;
    border: none;
    padding: 0;
    font: inherit;
    color: var(--accent);
    cursor: pointer;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
</style>
