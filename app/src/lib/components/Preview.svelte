<script lang="ts">
  import { checksLocally, describePlaces, filterLines, formatDuration, targetLabel } from "$lib/format";
  import { num, t } from "$lib/i18n.svelte";
  import { allCounted, allRead, countOf, deleteTimeMs, readProgress, totalOf, type ScanState } from "$lib/scan";
  import type { Filter, JobOptions } from "$lib/types";
  import Avatar from "./Avatar.svelte";
  import ConfirmDelete from "./ConfirmDelete.svelte";
  import ScanStatsView from "./ScanStatsView.svelte";
  import StatusLine from "./StatusLine.svelte";

  let {
    scan,
    filter,
    options,
    saving,
    onBack,
    onStop,
    onStart,
  }: {
    scan: ScanState;
    filter: Filter;
    options: JobOptions;
    /** What is saved before deleting, as set up (the only place to set it). */
    saving: { backup: boolean; export: boolean };
    onBack: () => void;
    onStop: () => void;
    onStart: (dryRun: boolean) => void;
  } = $props();

  let confirm: ConfirmDelete | undefined = $state();

  const names = $derived(new Map(scan.targets.map((t) => [t.id, t.name])));
  const placeName = (id: string) => names.get(id) ?? "";
  const total = $derived(totalOf(scan));
  const counted = $derived(allCounted(scan));
  const read = $derived(allRead(scan));
  const progress = $derived(readProgress(scan));
  const withMatches = $derived(scan.targets.filter((t) => countOf(scan.places[t.id]) > 0));
  const places = $derived(
    describePlaces(
      withMatches.filter((t) => t.kind === "guild").length,
      withMatches.filter((t) => t.kind !== "guild").length,
    ),
  );
  const conditions = $derived(filterLines(filter));
  const failures = $derived(scan.targets.filter((t) => scan.places[t.id]?.error).length);
  // Before everything is read, Discord's numbers include what local checks
  // would keep.
  const upTo = $derived(!read && (checksLocally(filter) || filter.skip_pinned));
  const unreadPages = $derived(Math.ceil(Math.max(0, progress.of - progress.read) / 25));
  const readLeftMs = $derived(unreadPages * (options.search_delay_ms + 600));
  const deleteMs = $derived(deleteTimeMs(total, options, read ? 0 : unreadPages));
  const title = $derived.by(() => {
    if (!counted) return t("preview.counting");
    if (scan.running) return t("preview.reading");
    return t("preview.title");
  });
</script>

<section class="card preview">
  <header>
    <div>
      <h2>{title}</h2>
      <ul class="conditions muted small">
        {#each conditions as line, i (i)}<li>{line}</li>{/each}
      </ul>
    </div>
    {#if scan.running}
      <button class="btn small" onclick={onStop} title={t("preview.stopHint")}>{t("preview.stop")}</button>
    {/if}
  </header>

  <StatusLine activity={scan.activity} waiting={scan.waiting} {placeName} busy={scan.running} />

  <div class="table" role="table">
    {#each scan.targets as target (target.id)}
      {@const p = scan.places[target.id]}
      <div class="row" role="row">
        <Avatar name={target.name} url={target.icon_url} size={24} />
        <span class="name" role="cell">
          {targetLabel(target)}
          {#if p && !p.complete && !p.error && (p.total ?? 0) > 0}
            <span class="read" aria-hidden="true">
              <span style:width="{Math.min(100, (p.read / Math.max(p.total ?? 1, 1)) * 100)}%"></span>
            </span>
          {/if}
        </span>
        <span class="value num" role="cell">
          {#if !p || (p.total === null && !p.error)}
            {#if scan.running}<span class="spinner muted"></span>{/if}
          {:else if p.error}
            <span class="failed" title={p.error}>{t("preview.couldNotSearch")}</span>
          {:else}
            <span class:zero={countOf(p) === 0} title={p.complete ? t("preview.exact") : t("preview.estimate")}>
              {p.complete || scan.exact ? "" : "≈ "}{num(countOf(p))}
            </span>
          {/if}
        </span>
      </div>
    {/each}
  </div>

  <div class="total">
    <span>{t("preview.total")}</span>
    <span class="num">{read || scan.exact ? "" : "≈ "}{num(total)}</span>
  </div>

  {#if !scan.exact && counted && !read && progress.of > 0}
    <div class="reading small muted">
      <div class="bar" aria-hidden="true"><span style:width="{(progress.read / progress.of) * 100}%"></span></div>
      <span>
        {t("preview.readOf", { read: num(progress.read), of: num(progress.of) })}
        {#if scan.running && readLeftMs > 0}· {t("preview.left", { time: formatDuration(readLeftMs) })}{/if}
      </span>
    </div>
  {/if}

  {#if scan.error}
    <p class="callout error small">{scan.error}</p>
  {/if}
  {#if counted && failures > 0}
    <p class="callout warn small">{t("preview.failures", { count: failures })}</p>
  {/if}
  {#if !scan.running && !read && !scan.exact && total > 0}
    <p class="small muted">{t("preview.partialNote")}</p>
  {/if}
  {#if scan.exact && total > 0}
    <p class="small muted">{t("preview.packageNote")}</p>
  {/if}

  {#if scan.stats && scan.stats.messages > 0}
    <ScanStatsView stats={scan.stats} partial={!read} deleteMs={counted ? deleteMs : null} />
  {/if}

  {#if counted && total > 0}
    <p class="small muted">
      {#if saving.backup || saving.export}
        {saving.backup && saving.export ? t("preview.savingBoth") : saving.backup ? t("preview.savingBackup") : t("preview.savingExport")}
        {t("preview.dryRunSaves")}
      {:else}
        {t("preview.savingNone")}
      {/if}
      <button class="link small" onclick={onBack}>
        {saving.backup || saving.export ? t("preview.savingChange") : t("preview.savingSetUp")}
      </button>
    </p>
  {/if}

  <footer>
    <button class="btn" onclick={onBack}>{t("common.back")}</button>
    <span class="spacer"></span>
    <button class="btn" onclick={() => onStart(true)} disabled={!counted || total === 0}>{t("preview.dryRun")}</button>
    <button class="btn danger" onclick={() => confirm?.open()} disabled={!counted || total === 0}>
      {total === 0 && counted ? t("preview.nothing") : t(upTo ? "preview.deleteUpTo" : "preview.delete", { count: total })}
    </button>
  </footer>
</section>

<ConfirmDelete bind:this={confirm} count={total} {upTo} {places} {filter} onConfirm={() => onStart(false)} />

<style>
  .preview {
    width: 100%;
    max-width: 860px;
    margin: 0 auto;
    padding: 20px;
    display: grid;
    gap: 14px;
  }

  header {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    gap: 12px;
  }

  header div {
    display: grid;
    gap: 4px;
  }

  h2 {
    margin: 0;
  }

  .conditions {
    margin: 0;
    padding-left: 18px;
  }

  .table {
    border: 1px solid var(--border);
    border-radius: 10px;
    max-height: 36vh;
    overflow-y: auto;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 12px;
    border-bottom: 1px solid var(--border);
  }

  .row:last-child {
    border-bottom: none;
  }

  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    display: grid;
    gap: 4px;
  }

  .read {
    display: block;
    height: 3px;
    border-radius: 2px;
    background: var(--panel-2);
    overflow: hidden;
    max-width: 220px;
  }

  .read span,
  .bar span {
    display: block;
    height: 100%;
    background: var(--accent);
    transition: width 0.3s;
  }

  .value {
    min-width: 80px;
    text-align: right;
    font-weight: 600;
    display: flex;
    justify-content: flex-end;
  }

  .zero {
    color: var(--muted);
    font-weight: 400;
  }

  .failed {
    color: var(--warn);
    font-weight: 500;
  }

  .total {
    display: flex;
    justify-content: space-between;
    padding: 0 12px;
    font-weight: 700;
    font-size: 16px;
  }

  .reading {
    display: grid;
    gap: 4px;
  }

  .bar {
    height: 6px;
    border-radius: 3px;
    background: var(--panel-2);
    overflow: hidden;
  }

  p {
    margin: 0;
  }

  .link {
    background: none;
    border: none;
    padding: 0;
    font: inherit;
    color: var(--accent);
    cursor: pointer;
  }

  footer {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
    padding-top: 6px;
  }

  .spacer {
    flex: 1;
  }

  @media (prefers-reduced-motion: reduce) {
    .read span,
    .bar span {
      transition: none;
    }
  }
</style>
