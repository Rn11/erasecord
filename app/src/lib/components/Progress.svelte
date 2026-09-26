<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { formatDuration, targetLabel } from "$lib/format";
  import { activeMs, processed, type RunState } from "$lib/run";
  import Avatar from "./Avatar.svelte";

  let {
    run,
    onPause,
    onResume,
    onStop,
    onDone,
  }: {
    run: RunState;
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
  const remaining = $derived(
    !finished && !paused && handled >= 5 && run.expected > handled ? (elapsed / handled) * (run.expected - handled) : null,
  );
  const title = $derived.by(() => {
    if (run.summary?.error) return "Stopped because of an error";
    if (run.summary?.cancelled) return "Stopped";
    if (finished) return run.dryRun ? "Dry run finished" : "Done";
    if (paused) return "Paused";
    return run.dryRun ? "Listing messages (dry run)…" : "Deleting…";
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

  const time = (at: number) => new Date(at).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit", second: "2-digit" });
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
          <button class="btn primary" onclick={onDone}>Start a new clean-up</button>
        {:else}
          {#if paused}
            <button class="btn" onclick={onResume}>Resume</button>
          {:else}
            <button class="btn" onclick={onPause}>Pause</button>
          {/if}
          <button class="btn" onclick={onStop}>Stop</button>
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
        <span class="value num">{run.totals.deleted.toLocaleString()}</span>
        <span class="label">{run.dryRun ? "would be deleted" : "deleted"}</span>
      </div>
      <div class="stat">
        <span class="value num">{run.totals.skipped.toLocaleString()}</span>
        <span class="label">skipped</span>
      </div>
      <div class="stat" class:bad={run.totals.failed > 0}>
        <span class="value num">{run.totals.failed.toLocaleString()}</span>
        <span class="label">failed</span>
      </div>
      <div class="stat time">
        <span class="value num">{formatDuration(elapsed)}</span>
        <span class="label">
          {#if remaining !== null}about {formatDuration(remaining)} left{:else}elapsed{/if}
        </span>
      </div>
    </div>

    {#if run.summary?.error}
      <p class="callout error small">{run.summary.error}</p>
    {:else if finished && run.dryRun}
      <p class="callout info small">
        This was a dry run: nothing was deleted. The activity log lists every message that would be deleted.
      </p>
    {/if}
  </section>

  <div class="columns">
    <section class="card targets" aria-label="Servers and DMs">
      {#each run.targets as item (item.target.id)}
        <div class="target" class:active={item.status === "running"}>
          <Avatar name={item.target.name} url={item.target.icon_url} size={24} />
          <span class="name" title={item.error ?? undefined}>{targetLabel(item.target)}</span>
          <span class="state small num">
            {#if item.status === "pending"}
              <span class="muted">{finished ? "not started" : "waiting"}</span>
            {:else if item.status === "failed"}
              <span class="warn">could not search</span>
            {:else}
              {item.stats.deleted.toLocaleString()}{#if item.estimate !== null}<span class="muted"
                  >&nbsp;/&nbsp;{item.estimate.toLocaleString()}</span
                >{/if}
              {#if item.status === "done"}<span class="check" aria-label="done">✓</span>{/if}
              {#if item.status === "stopped"}<span class="muted">· stopped</span>{/if}
            {/if}
          </span>
        </div>
      {/each}
    </section>

    <section class="card log" aria-label="Activity">
      <h3>Activity</h3>
      <div class="lines" bind:this={logBox} onscroll={onScroll}>
        {#each run.log as line (line.id)}
          <div class="line {line.tone}">
            <span class="at num">{time(line.at)}</span>
            <span class="text">{line.text}</span>
          </div>
        {:else}
          <p class="muted small">Starting…</p>
        {/each}
      </div>
    </section>
  </div>
</div>

<style>
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
