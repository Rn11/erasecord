<script lang="ts">
  import { checksLocally, describePlaces, filterLines, targetLabel } from "$lib/format";
  import { num, t } from "$lib/i18n.svelte";
  import type { Filter, PreviewEntry, Target } from "$lib/types";
  import Avatar from "./Avatar.svelte";

  let {
    targets,
    entries,
    counting,
    exact = false,
    error,
    filter,
    onBack,
    onCancel,
    onStart,
  }: {
    targets: Target[];
    entries: PreviewEntry[];
    counting: boolean;
    /** Counts come from the data package: exact, no search involved. */
    exact?: boolean;
    error: string | null;
    filter: Filter;
    onBack: () => void;
    onCancel: () => void;
    onStart: (dryRun: boolean) => void;
  } = $props();

  let dialog: HTMLDialogElement | undefined = $state();

  const byId = $derived(new Map(entries.map((e) => [e.target.id, e])));
  const total = $derived(entries.reduce((sum, e) => sum + (e.count ?? 0), 0));
  const withMatches = $derived(entries.filter((e) => (e.count ?? 0) > 0));
  const places = $derived(
    describePlaces(
      withMatches.filter((e) => e.target.kind === "guild").length,
      withMatches.filter((e) => e.target.kind !== "guild").length,
    ),
  );
  const conditions = $derived(filterLines(filter));
  const failures = $derived(entries.filter((e) => e.error).length);
  const approximate = $derived(!exact && checksLocally(filter));
  const upTo = $derived(approximate || filter.skip_pinned);

  function confirmDelete() {
    dialog?.close();
    onStart(false);
  }
</script>

<section class="card preview">
  <header>
    <div>
      <h2>{counting ? t("preview.counting") : t("preview.title")}</h2>
      <p class="muted">{t("preview.intro")}</p>
      <ul class="conditions muted small">
        {#each conditions as line, i (i)}<li>{line}</li>{/each}
      </ul>
    </div>
    {#if counting}
      <span class="muted small num">{num(entries.length)} / {num(targets.length)}</span>
    {/if}
  </header>

  <div class="table" role="table">
    {#each targets as target (target.id)}
      {@const entry = byId.get(target.id)}
      <div class="row" role="row">
        <Avatar name={target.name} url={target.icon_url} size={24} />
        <span class="name" role="cell">{targetLabel(target)}</span>
        <span class="value num" role="cell">
          {#if !entry}
            {#if counting}<span class="spinner muted"></span>{/if}
          {:else if entry.error}
            <span class="failed" title={entry.error}>{t("preview.couldNotSearch")}</span>
          {:else}
            <span class:zero={entry.count === 0}>{num(entry.count ?? 0)}</span>
          {/if}
        </span>
      </div>
    {/each}
  </div>

  <div class="total">
    <span>{t("preview.total")}</span>
    <span class="num">{num(total)}</span>
  </div>

  {#if error}
    <p class="callout error small">{error}</p>
  {/if}
  {#if !counting && failures > 0}
    <p class="callout warn small">{t("preview.failures", { count: failures })}</p>
  {/if}
  {#if !counting && filter.skip_pinned && total > 0}
    <p class="small muted">{t("preview.pinnedNote")}</p>
  {/if}
  {#if !counting && exact && total > 0}
    <p class="small muted">{t("preview.packageNote")}</p>
  {/if}
  {#if !counting && approximate && total > 0}
    <p class="small muted">{t("preview.approxNote")}</p>
  {/if}

  <footer>
    {#if counting}
      <button class="btn" onclick={onCancel}>{t("common.cancel")}</button>
    {:else}
      <button class="btn" onclick={onBack}>{t("common.back")}</button>
      <span class="spacer"></span>
      <button class="btn" onclick={() => onStart(true)} disabled={total === 0}>{t("preview.dryRun")}</button>
      <button class="btn danger" onclick={() => dialog?.showModal()} disabled={total === 0}>
        {total === 0 ? t("preview.nothing") : t(upTo ? "preview.deleteUpTo" : "preview.delete", { count: total })}
      </button>
    {/if}
  </footer>
</section>

<dialog bind:this={dialog} class="card confirm" aria-labelledby="confirm-title">
  <h2 id="confirm-title">{t(upTo ? "confirm.titleUpTo" : "confirm.title", { count: total })}</h2>
  <ul class="conditions small">
    {#each conditions as line, i (i)}<li>{line}</li>{/each}
  </ul>
  <p>{t("confirm.body", { places })} <strong>{t("confirm.undo")}</strong></p>
  <p class="muted small">{t("confirm.slow")}</p>
  <div class="actions">
    <button class="btn" onclick={() => dialog?.close()}>{t("common.cancel")}</button>
    <button class="btn danger" onclick={confirmDelete}>{t("confirm.go")}</button>
  </div>
</dialog>

<style>
  .preview {
    width: 100%;
    max-width: 720px;
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

  .conditions {
    margin: 0;
    padding-left: 18px;
  }

  .table {
    border: 1px solid var(--border);
    border-radius: 10px;
    max-height: 48vh;
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

  footer {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
    padding-top: 6px;
  }

  .spacer {
    flex: 1;
  }

  .confirm {
    width: min(440px, calc(100% - 32px));
    /* WebKitGTK would otherwise stretch the modal to the full window height. */
    height: fit-content;
    max-height: calc(100vh - 32px);
    padding: 22px;
    color: var(--text);
    border: 1px solid var(--border);
  }

  .confirm[open] {
    display: grid;
    align-content: start;
    gap: 12px;
  }

  .confirm::backdrop {
    background: rgb(0 0 0 / 0.45);
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 6px;
  }
</style>
