<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { api } from "$lib/api";
  import { t } from "$lib/i18n.svelte";
  import { newChoice, strength, strengthLevel, type PassphraseChoice } from "$lib/passphrase";

  // Picks the passphrase for an encrypted backup or export: twelve generated
  // words (confirmed by typing three of them), your own, or a file.
  let { choice = $bindable() }: { choice: PassphraseChoice } = $props();

  let show = $state(false);
  let copied = $state(false);
  const words = $derived(choice.words.split(" "));
  const bits = $derived(strength(choice.own));
  const level = $derived(strengthLevel(bits));

  async function regenerate() {
    const mode = choice.mode;
    choice = { ...newChoice(await api.generatePassphrase()), mode };
    copied = false;
  }

  async function copy() {
    try {
      await navigator.clipboard.writeText(choice.words);
      copied = true;
    } catch {
      copied = false;
    }
  }

  async function pickFile() {
    try {
      const file = await open({ multiple: false, directory: false, title: t("pass.pickFile") });
      if (typeof file === "string") choice.file = file;
    } catch {
      // Cancelled or not available.
    }
  }
</script>

<div class="passphrase small">
  <div class="modes" role="radiogroup" aria-label={t("pass.label")}>
    <label><input type="radio" value="generated" bind:group={choice.mode} /> {t("pass.generated")}</label>
    <label><input type="radio" value="own" bind:group={choice.mode} /> {t("pass.own")}</label>
    <label><input type="radio" value="file" bind:group={choice.mode} /> {t("pass.file")}</label>
  </div>

  {#if choice.mode === "generated"}
    <ol class="words" aria-label={t("pass.generated")}>
      {#each words as word, i (i)}
        <li><span class="n muted">{i + 1}</span> <code>{word}</code></li>
      {/each}
    </ol>
    <div class="row">
      <button type="button" class="btn small" onclick={copy}>{copied ? t("pass.copied") : t("pass.copy")}</button>
      <button type="button" class="btn small ghost" onclick={regenerate}>{t("pass.newWords")}</button>
    </div>
    <p class="warn">{t("pass.writeDown")}</p>
    <div class="checks">
      <span>{t("pass.confirm")}</span>
      {#each choice.checks as n, i (n)}
        <label class="check">
          <span class="muted">{t("pass.word", { n })}</span>
          <input
            type="text"
            autocomplete="off"
            spellcheck="false"
            bind:value={choice.answers[i]}
            class:ok={choice.answers[i].trim().toLowerCase() === words[n - 1]}
          />
        </label>
      {/each}
    </div>
  {:else if choice.mode === "own"}
    <label class="field">
      <span class="muted">{t("pass.label")}</span>
      <span class="with-toggle">
        <input type={show ? "text" : "password"} autocomplete="new-password" bind:value={choice.own} />
        <button type="button" class="link" onclick={() => (show = !show)}>{show ? t("login.hide") : t("login.show")}</button>
      </span>
    </label>
    {#if choice.own}
      <div class="meter" aria-label={t("pass.strength")}>
        <span class="bar {level}" style:width="{Math.min(100, (bits / 90) * 100)}%"></span>
      </div>
      <span class="muted">{t(`pass.${level}`)}</span>
    {/if}
    <label class="field">
      <span class="muted">{t("pass.repeat")}</span>
      <input type={show ? "text" : "password"} autocomplete="new-password" bind:value={choice.repeat} />
    </label>
  {:else}
    <div class="row">
      <button type="button" class="btn small" onclick={pickFile}>{t("pass.pickFile")}</button>
      {#if choice.file}<code class="path" title={choice.file}>{choice.file}</code>{/if}
    </div>
    <p class="muted">{t("pass.fileHint")}</p>
  {/if}
</div>

<style>
  .passphrase {
    display: grid;
    gap: 8px;
  }

  .modes {
    display: flex;
    flex-wrap: wrap;
    gap: 4px 14px;
  }

  .modes label {
    display: inline-flex;
    align-items: center;
    gap: 5px;
  }

  .words {
    list-style: none;
    margin: 0;
    padding: 10px;
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 6px 10px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--panel-2);
  }

  .words li {
    display: flex;
    gap: 6px;
    align-items: baseline;
  }

  .n {
    width: 16px;
    text-align: right;
    font-size: 11px;
  }

  code {
    font-size: 13px;
  }

  .row {
    display: flex;
    gap: 8px;
    align-items: center;
    flex-wrap: wrap;
  }

  p {
    margin: 0;
  }

  .warn {
    color: var(--warn);
  }

  .checks {
    display: flex;
    flex-wrap: wrap;
    align-items: end;
    gap: 8px;
  }

  .check {
    display: grid;
    gap: 2px;
  }

  .check input {
    width: 110px;
  }

  .check input.ok {
    border-color: var(--ok);
  }

  .field {
    display: grid;
    gap: 3px;
  }

  .with-toggle {
    display: flex;
    gap: 8px;
    align-items: center;
  }

  .with-toggle input {
    flex: 1;
  }

  .link {
    background: none;
    border: none;
    padding: 0;
    font: inherit;
    color: var(--accent);
    cursor: pointer;
  }

  .meter {
    height: 6px;
    border-radius: 3px;
    background: var(--panel-2);
    overflow: hidden;
  }

  .bar {
    display: block;
    height: 100%;
  }

  .bar.weak {
    background: var(--danger);
  }

  .bar.fair {
    background: var(--warn);
  }

  .bar.strong {
    background: var(--ok);
  }

  .path {
    max-width: 260px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
