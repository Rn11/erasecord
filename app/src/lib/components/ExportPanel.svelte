<script lang="ts">
  import { save } from "@tauri-apps/plugin-dialog";
  import { api } from "$lib/api";
  import { errorMessage } from "$lib/errors";
  import { num, t } from "$lib/i18n.svelte";
  import { newChoice, passphraseInput, passphraseProblem, type PassphraseChoice } from "$lib/passphrase";
  import type { PassphraseInput } from "$lib/types";
  import Passphrase from "./Passphrase.svelte";

  // Saving messages to a file: encrypted by default, with the passphrase
  // already chosen for this clean-up if there is one.
  let {
    name,
    knownPassphrase = null,
    write,
    onClose,
  }: {
    /** The file name to suggest, without extension. */
    name: string;
    knownPassphrase?: PassphraseInput | null;
    /** Writes the file; returns the number of messages. */
    write: (path: string, passphrase: PassphraseInput | null) => Promise<number>;
    onClose: () => void;
  } = $props();

  let encrypt = $state(true);
  let choice = $state<PassphraseChoice | null>(null);
  let busy = $state(false);
  let result = $state<{ rows: number; path: string } | { error: string } | null>(null);

  $effect(() => {
    if (encrypt && !knownPassphrase && !choice) {
      api
        .generatePassphrase()
        .then((words) => (choice = newChoice(words)))
        .catch(() => (choice = null));
    }
  });

  const problem = $derived(
    encrypt && !knownPassphrase ? (choice ? passphraseProblem(choice) : t("pass.confirmNeeded")) : null,
  );

  async function saveFile() {
    result = null;
    const passphrase = encrypt ? (knownPassphrase ?? (choice ? passphraseInput(choice) : null)) : null;
    try {
      const path = await save({
        title: t("export.saveTitle"),
        defaultPath: `${name}.csv${passphrase ? ".age" : ""}`,
        filters: passphrase
          ? [{ name: t("progress.encryptedList"), extensions: ["age"] }]
          : [
              { name: t("progress.csv"), extensions: ["csv"] },
              { name: "JSON", extensions: ["json", "jsonl"] },
            ],
      });
      if (!path) return;
      busy = true;
      result = { rows: await write(path, passphrase), path };
    } catch (err) {
      result = { error: errorMessage(err) };
    } finally {
      busy = false;
    }
  }
</script>

<div class="panel small">
  <label class="choice">
    <input type="checkbox" bind:checked={encrypt} />
    <span>{t("export.encrypt")}</span>
  </label>
  {#if encrypt && knownPassphrase}
    <span class="muted">{t("export.samePassphrase")}</span>
  {:else if encrypt && choice}
    <Passphrase bind:choice />
  {:else if !encrypt}
    <span class="warn-text">{t("progress.plainList")}</span>
  {/if}
  {#if problem}<span class="muted">{problem}</span>{/if}
  {#if result && "error" in result}
    <p class="callout error">{result.error}</p>
  {:else if result}
    <p class="callout info">{t("progress.saved", { count: num(result.rows), path: result.path })}</p>
  {/if}
  <div class="actions">
    <button class="btn small ghost" onclick={onClose}>{result && !("error" in result) ? t("common.close") : t("common.cancel")}</button>
    <button class="btn small primary" onclick={saveFile} disabled={busy || !!problem}>
      {#if busy}<span class="spinner"></span>{/if}
      {t("export.save")}
    </button>
  </div>
</div>

<style>
  .panel {
    display: grid;
    gap: 8px;
    padding: 12px;
    border: 1px solid var(--border);
    border-radius: 10px;
  }

  .choice {
    display: flex;
    gap: 8px;
    align-items: center;
  }

  .warn-text {
    color: var(--warn);
  }

  p {
    margin: 0;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
</style>
