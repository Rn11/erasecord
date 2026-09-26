<script lang="ts">
  import { describeFilter, plural } from "$lib/format";
  import type { Filter, PreviewEntry, Target } from "$lib/types";
  import Avatar from "./Avatar.svelte";

  let {
    targets,
    entries,
    counting,
    error,
    filter,
    onBack,
    onCancel,
    onStart,
  }: {
    targets: Target[];
    entries: PreviewEntry[];
    counting: boolean;
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
    [
      plural(withMatches.filter((e) => e.target.kind === "guild").length, "server"),
      plural(withMatches.filter((e) => e.target.kind !== "guild").length, "DM"),
    ]
      .filter((text) => !text.startsWith("0 "))
      .join(" and "),
  );
  const failures = $derived(entries.filter((e) => e.error).length);

  function confirmDelete() {
    dialog?.close();
    onStart(false);
  }
</script>

<section class="card preview">
  <header>
    <div>
      <h2>{counting ? "Counting your messages…" : "Preview"}</h2>
      <p class="muted">Your messages {describeFilter(filter)}.</p>
    </div>
    {#if counting}
      <span class="muted small num">{entries.length} / {targets.length}</span>
    {/if}
  </header>

  <div class="table" role="table">
    {#each targets as target (target.id)}
      {@const entry = byId.get(target.id)}
      <div class="row" role="row">
        <Avatar name={target.name} url={target.icon_url} size={24} />
        <span class="name" role="cell">{target.name}</span>
        <span class="value num" role="cell">
          {#if !entry}
            {#if counting}<span class="spinner muted"></span>{/if}
          {:else if entry.error}
            <span class="failed" title={entry.error}>could not search</span>
          {:else}
            <span class:zero={entry.count === 0}>{(entry.count ?? 0).toLocaleString()}</span>
          {/if}
        </span>
      </div>
    {/each}
  </div>

  <div class="total">
    <span>Total</span>
    <span class="num">{total.toLocaleString()}</span>
  </div>

  {#if error}
    <p class="callout error small">{error}</p>
  {/if}
  {#if !counting && failures > 0}
    <p class="callout warn small">
      {plural(failures, "place")} could not be searched, usually because you no longer have access. purgecord tries again
      when you start.
    </p>
  {/if}
  {#if !counting && filter.skip_pinned && total > 0}
    <p class="small muted">Pinned messages are included in these numbers and will be kept.</p>
  {/if}

  <footer>
    {#if counting}
      <button class="btn" onclick={onCancel}>Cancel</button>
    {:else}
      <button class="btn" onclick={onBack}>Back</button>
      <span class="spacer"></span>
      <button class="btn" onclick={() => onStart(true)} disabled={total === 0}>List them first (dry run)</button>
      <button class="btn danger" onclick={() => dialog?.showModal()} disabled={total === 0}>
        {total === 0 ? "Nothing to delete" : `Delete ${plural(total, "message")}`}
      </button>
    {/if}
  </footer>
</section>

<dialog bind:this={dialog} class="card confirm" aria-labelledby="confirm-title">
  <h2 id="confirm-title">Delete {plural(total, "message")}?</h2>
  <p>
    Your messages {describeFilter(filter)} will be deleted from {places}. <strong>This cannot be undone.</strong>
  </p>
  <p class="muted small">
    This can take a while: purgecord deletes one message at a time. You can pause or stop at any point.
  </p>
  <div class="actions">
    <button class="btn" onclick={() => dialog?.close()}>Cancel</button>
    <button class="btn danger" onclick={confirmDelete}>Delete permanently</button>
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
