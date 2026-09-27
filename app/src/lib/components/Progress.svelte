<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { save } from "@tauri-apps/plugin-dialog";
  import { api } from "$lib/api";
  import { errorMessage } from "$lib/errors";
  import { formatDuration, targetLabel } from "$lib/format";
  import { i18n, num, t } from "$lib/i18n.svelte";
  import { activeMs, processed, type RunState } from "$lib/run";
  import { newChoice, passphraseInput, passphraseProblem, type PassphraseChoice } from "$lib/passphrase";
  import { revealItemInDir } from "@tauri-apps/plugin-opener";
  import type { PassphraseInput } from "$lib/types";
  import Avatar from "./Avatar.svelte";
  import Passphrase from "./Passphrase.svelte";

  let {
    run,
    backupPassphrase = null,
    onPause,
    onResume,
    onStop,
    onDone,
  }: {
    run: RunState;
    /** The passphrase of this run's encrypted backup, reused for its list. */
    backupPassphrase?: PassphraseInput | null;
    onPause: () => void;
    onResume: () => void;
    onStop: () => void;
    onDone: () => void;
  } = $props();

  let now = $state(Date.now());
  let timer: ReturnType<typeof setInterval> | undefined;
  onMount(() => {
    timer = setInterval(() => (now = Date.now()), 1000);
  });
  onDestroy(() => clearInterval(timer));

  const handled = $derived(processed(run.totals));
  const finished = $derived(run.summary !== null);
  const paused = $derived(run.pausedAt !== null);
  const completed = $derived(finished && !run.summary?.cancelled && !run.summary?.error);
  const fraction = $derived(completed ? 1 : run.expected > 0 ? Math.min(handled / run.expected, 1) : 0);
  const elapsed = $derived(activeMs(run, now));
  // Only this session's work tells how fast it goes.
  const handledNow = $derived(handled - processed(run.prior));
  const remaining = $derived(
    !finished && !paused && handledNow >= 5 && run.expected > handled
      ? (elapsed / handledNow) * (run.expected - handled)
      : null,
  );
  const title = $derived.by(() => {
    if (run.summary?.error) return t("progress.error");
    if (run.summary?.cancelled) return t("progress.stopped");
    if (finished) return run.dryRun ? t("progress.dryDone") : t("progress.done");
    if (paused) return t("progress.paused");
    return run.dryRun ? t("progress.dryRunning") : t("progress.running");
  });

  let logBox: HTMLDivElement | undefined = $state();
  let follow = $state(true);

  $effect(() => {
    // Keep the newest line in view unless the user scrolled up.
    void run.log.length;
    if (follow && logBox) logBox.scrollTop = logBox.scrollHeight;
  });

  function onScroll() {
    if (logBox) follow = logBox.scrollHeight - logBox.scrollTop - logBox.clientHeight < 40;
  }

  let exporting = $state(false);
  let exportResult = $state<{ rows: number; path: string } | { error: string } | null>(null);

  // Saving the list: encrypted by default, with the backup's passphrase if
  // the run had an encrypted backup.
  let choosing = $state(false);
  let encryptList = $state(true);
  let listChoice = $state<PassphraseChoice | null>(null);
  const listProblem = $derived(
    encryptList && !backupPassphrase ? (listChoice ? passphraseProblem(listChoice) : t("pass.confirmNeeded")) : null,
  );

  async function startSaving() {
    choosing = true;
    exportResult = null;
    if (!backupPassphrase && !listChoice) {
      try {
        listChoice = newChoice(await api.generatePassphrase());
      } catch {
        listChoice = null;
      }
    }
  }

  async function exportList() {
    exportResult = null;
    const day = new Date().toISOString().slice(0, 10);
    const passphrase = encryptList ? (backupPassphrase ?? (listChoice ? passphraseInput(listChoice) : null)) : null;
    const name = `erasecord-${run.dryRun ? "dry-run" : "deleted"}-${day}.csv${passphrase ? ".age" : ""}`;
    try {
      const path = await save({
        title: t("progress.saveTitle"),
        defaultPath: name,
        filters: passphrase
          ? [{ name: t("progress.encryptedList"), extensions: ["age"] }]
          : [
              { name: t("progress.csv"), extensions: ["csv"] },
              { name: "JSON", extensions: ["json"] },
            ],
      });
      if (!path) return;
      exporting = true;
      const rows = await api.exportRun(path, passphrase);
      choosing = false;
      exportResult = { rows, path };
    } catch (err) {
      exportResult = { error: errorMessage(err) };
    } finally {
      exporting = false;
    }
  }

  const time = (at: number) => new Date(at).toLocaleTimeString(i18n.locale, { hour: "2-digit", minute: "2-digit", second: "2-digit" });
</script>

<div class="progress">
  <section class="card head">
    <div class="title-row">
      <h2>
        {#if !finished && !paused}<span class="spinner"></span>{/if}
        {title}
      </h2>
      <div class="actions">
        {#if finished}
          <button
            class="btn"
            onclick={startSaving}
            disabled={exporting || choosing || run.totals.deleted === 0}
            title={t("progress.saveHint")}
          >
            {#if exporting}<span class="spinner"></span>{/if} {t("progress.saveList")}
          </button>
          <button class="btn primary" onclick={onDone}>{t("progress.newRun")}</button>
        {:else}
          {#if paused}
            <button class="btn" onclick={onResume}>{t("progress.resume")}</button>
          {:else}
            <button class="btn" onclick={onPause}>{t("progress.pause")}</button>
          {/if}
          <button class="btn" onclick={onStop}>{t("progress.stop")}</button>
        {/if}
      </div>
    </div>

    <div
      class="bar"
      role="progressbar"
      aria-valuemin="0"
      aria-valuemax="100"
      aria-valuenow={Math.round(fraction * 100)}
      class:paused
    >
      <div class="fill" style:width="{fraction * 100}%"></div>
    </div>

    <div class="stats">
      <div class="stat ok">
        <span class="value num">{num(run.totals.deleted)}</span>
        <span class="label">{run.dryRun ? t("progress.wouldDelete") : t("progress.deleted")}</span>
      </div>
      <div class="stat">
        <span class="value num">{num(run.totals.skipped)}</span>
        <span class="label">{t("progress.skipped")}</span>
      </div>
      <div class="stat" class:bad={run.totals.failed > 0}>
        <span class="value num">{num(run.totals.failed)}</span>
        <span class="label">{t("progress.failed")}</span>
      </div>
      <div class="stat time">
        <span class="value num">{formatDuration(elapsed)}</span>
        <span class="label">
          {#if remaining !== null}{t("progress.left", { time: formatDuration(remaining) })}{:else}{t("progress.elapsed")}{/if}
        </span>
      </div>
    </div>

    {#if run.backup && "archive" in run.backup}
      <p class="callout small info backup">
        <span>{t("progress.backupSealed", { files: num(run.backup.files), messages: num(run.backup.messages) })}</span>
        <code title={run.backup.archive}>{run.backup.archive}</code>
        <button class="link" onclick={() => run.backup && "archive" in run.backup && revealItemInDir(run.backup.archive)}>
          {t("open.show")}
        </button>
      </p>
    {:else if run.backup}
      <p class="callout small warn">{t("progress.backupKept", { reason: run.backup.reason })}</p>
    {/if}
    {#if choosing}
      <div class="save-panel small">
        <label class="choice">
          <input type="checkbox" bind:checked={encryptList} />
          <span>{t("progress.encryptList")}</span>
        </label>
        {#if encryptList && backupPassphrase}
          <span class="muted">{t("progress.sameAsBackup")}</span>
        {:else if encryptList && listChoice}
          <Passphrase bind:choice={listChoice} />
        {:else if !encryptList}
          <span class="warn-text">{t("progress.plainList")}</span>
        {/if}
        {#if listProblem}<span class="muted">{listProblem}</span>{/if}
        <div class="save-actions">
          <button class="btn small ghost" onclick={() => (choosing = false)}>{t("common.cancel")}</button>
          <button class="btn small primary" onclick={exportList} disabled={exporting || !!listProblem}>
            {#if exporting}<span class="spinner"></span>{/if}
            {t("progress.saveList")}
          </button>
        </div>
      </div>
    {/if}
    {#if exportResult && "error" in exportResult}
      <p class="callout small error">{exportResult.error}</p>
    {:else if exportResult}
      <p class="callout small info">{t("progress.saved", { count: exportResult.rows, path: exportResult.path })}</p>
    {/if}
    {#if run.summary?.error}
      <p class="callout error small">{run.summary.error}</p>
    {:else if finished && run.dryRun}
      <p class="callout info small">{t("progress.dryNote")}</p>
    {/if}
  </section>

  <div class="columns">
    <section class="card targets" aria-label={t("progress.targetsLabel")}>
      {#each run.targets as item (item.target.id)}
        <div class="target" class:active={item.status === "running"}>
          <Avatar name={item.target.name} url={item.target.icon_url} size={24} />
          <span class="name" title={item.error ?? undefined}>{targetLabel(item.target)}</span>
          <span class="state small num">
            {#if item.status === "pending"}
              <span class="muted">{finished ? t("progress.notStarted") : t("progress.waiting")}</span>
            {:else if item.status === "failed"}
              <span class="warn">{t("progress.couldNotSearch")}</span>
            {:else}
              {num(item.stats.deleted)}{#if item.estimate !== null}<span class="muted">&nbsp;/&nbsp;{num(item.estimate)}</span>{/if}
              {#if item.status === "done"}<span class="check" aria-label={t("progress.doneTag")}>✓</span>{/if}
              {#if item.status === "stopped"}<span class="muted">· {t("progress.stoppedTag")}</span>{/if}
            {/if}
          </span>
        </div>
      {/each}
    </section>

    <section class="card log" aria-label={t("progress.activity")}>
      <h3>{t("progress.activity")}</h3>
      <div class="lines" bind:this={logBox} onscroll={onScroll}>
        {#each run.log as line (line.id)}
          <div class="line {line.tone}">
            <span class="at num">{time(line.at)}</span>
            <span class="text">{line.text}</span>
          </div>
        {:else}
          <p class="muted small">{t("common.starting")}</p>
        {/each}
      </div>
    </section>
  </div>
</div>

<style>
  .backup {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px 10px;
  }

  .backup code {
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

  .save-panel {
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

  .save-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }

  .progress {
    display: grid;
    grid-template-rows: auto 1fr;
    gap: 16px;
    height: 100%;
    min-height: 0;
  }

  .head {
    padding: 18px 20px;
    display: grid;
    gap: 14px;
  }

  .title-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 12px;
    flex-wrap: wrap;
  }

  h2 {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .actions {
    display: flex;
    gap: 8px;
  }

  .bar {
    height: 8px;
    border-radius: 999px;
    background: var(--panel-2);
    overflow: hidden;
  }

  .fill {
    height: 100%;
    background: var(--accent);
    border-radius: inherit;
    transition: width 0.4s ease;
  }

  .bar.paused .fill {
    background: var(--muted);
  }

  .stats {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: 12px;
  }

  @media (max-width: 640px) {
    .stats {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
  }

  .stat {
    display: grid;
    gap: 2px;
  }

  .stat .value {
    font-size: 22px;
    font-weight: 700;
  }

  .stat.ok .value {
    color: var(--ok);
  }

  .stat.bad .value {
    color: var(--danger);
  }

  .stat.time .value {
    font-size: 16px;
    padding-top: 5px;
  }

  .label {
    color: var(--muted);
    font-size: 13px;
  }

  .columns {
    display: grid;
    grid-template-columns: minmax(240px, 320px) 1fr;
    gap: 16px;
    min-height: 0;
  }

  @media (max-width: 760px) {
    .columns {
      grid-template-columns: 1fr;
    }
  }

  .targets {
    overflow-y: auto;
    padding: 6px;
    min-height: 160px;
  }

  .target {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 7px 8px;
    border-radius: 8px;
  }

  .target.active {
    background: var(--accent-soft);
  }

  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .state {
    display: flex;
    align-items: center;
    gap: 4px;
    white-space: nowrap;
  }

  .warn {
    color: var(--warn);
  }

  .check {
    color: var(--ok);
    font-weight: 700;
  }

  .log {
    display: flex;
    flex-direction: column;
    min-height: 200px;
    overflow: hidden;
  }

  .log h3 {
    padding: 12px 14px;
    border-bottom: 1px solid var(--border);
  }

  .lines {
    flex: 1;
    overflow-y: auto;
    padding: 8px 14px;
    font-size: 12.5px;
    font-family: ui-monospace, "SF Mono", Menlo, Consolas, monospace;
  }

  .line {
    display: flex;
    gap: 10px;
    padding: 1px 0;
  }

  .at {
    color: var(--muted);
    flex: none;
  }

  .text {
    min-width: 0;
    overflow-wrap: anywhere;
  }

  .line.muted .text {
    color: var(--muted);
  }

  .line.warn .text {
    color: var(--warn);
  }

  .line.error .text {
    color: var(--danger);
  }
</style>
