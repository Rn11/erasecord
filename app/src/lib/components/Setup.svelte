<script lang="ts">
  import type { SvelteSet } from "svelte/reactivity";
  import {
    HAS_KINDS,
    contentProblem,
    describeFilter,
    plural,
    rangeProblem,
    toFilter,
    type ContentForm,
    type RangeForm,
  } from "$lib/format";
  import type { Has, JobOptions, Target } from "$lib/types";
  import Avatar from "./Avatar.svelte";

  let {
    targets,
    loading,
    error,
    selected,
    range = $bindable(),
    content = $bindable(),
    skipPinned = $bindable(),
    options = $bindable(),
    onReload,
    onCount,
  }: {
    targets: Target[];
    loading: boolean;
    error: string | null;
    selected: SvelteSet<string>;
    range: RangeForm;
    content: ContentForm;
    skipPinned: boolean;
    options: JobOptions;
    onReload: () => void;
    onCount: () => void;
  } = $props();

  let tab = $state<"servers" | "dms">("servers");
  let query = $state("");

  const servers = $derived(targets.filter((t) => t.kind === "guild"));
  const dms = $derived(targets.filter((t) => t.kind !== "guild"));
  const visible = $derived(
    (tab === "servers" ? servers : dms).filter((t) => t.name.toLowerCase().includes(query.trim().toLowerCase())),
  );
  const allVisibleSelected = $derived(visible.length > 0 && visible.every((t) => selected.has(t.id)));
  const selectedServers = $derived(servers.filter((t) => selected.has(t.id)).length);
  const selectedDms = $derived(dms.filter((t) => selected.has(t.id)).length);
  const problem = $derived(rangeProblem(range) ?? contentProblem(content));
  const summary = $derived(problem ? null : describeFilter(toFilter(range, skipPinned, content)));
  const contentActive = $derived(
    !!content.contains.trim() || !!content.pattern.trim() || content.has.length > 0 || content.without.length > 0,
  );

  function toggleKind(list: "has" | "without", kind: Has) {
    const current = content[list];
    content[list] = current.includes(kind) ? current.filter((k) => k !== kind) : [...current, kind];
  }

  function toggle(id: string) {
    if (selected.has(id)) selected.delete(id);
    else selected.add(id);
  }

  function toggleVisible() {
    const select = !allVisibleSelected;
    for (const target of visible) {
      if (select) selected.add(target.id);
      else selected.delete(target.id);
    }
  }
</script>

<div class="setup">
  <section class="card picker" aria-label="Servers and direct messages">
    <div class="tabs" role="tablist">
      <button role="tab" aria-selected={tab === "servers"} class:active={tab === "servers"} onclick={() => (tab = "servers")}>
        Servers <span class="count">{selectedServers ? `${selectedServers}/` : ""}{servers.length}</span>
      </button>
      <button role="tab" aria-selected={tab === "dms"} class:active={tab === "dms"} onclick={() => (tab = "dms")}>
        Direct messages <span class="count">{selectedDms ? `${selectedDms}/` : ""}{dms.length}</span>
      </button>
    </div>

    <div class="toolbar">
      <input type="search" placeholder="Filter by name" bind:value={query} aria-label="Filter by name" />
      <button class="btn small" onclick={toggleVisible} disabled={visible.length === 0}>
        {allVisibleSelected ? "Select none" : "Select all"}
      </button>
      <button class="btn small ghost" onclick={onReload} disabled={loading} title="Reload list">↻</button>
    </div>

    <div class="list">
      {#if loading && targets.length === 0}
        <p class="empty muted"><span class="spinner"></span> Loading your servers and DMs…</p>
      {:else if error}
        <div class="empty">
          <p class="callout error small">{error}</p>
          <button class="btn small" onclick={onReload}>Try again</button>
        </div>
      {:else if visible.length === 0}
        <p class="empty muted">{query ? "Nothing matches your filter." : tab === "servers" ? "You are not in any server." : "No open DMs."}</p>
      {:else}
        {#each visible as target (target.id)}
          <label class="row" class:checked={selected.has(target.id)}>
            <input type="checkbox" checked={selected.has(target.id)} onchange={() => toggle(target.id)} />
            <Avatar name={target.name} url={target.icon_url} size={28} />
            <span class="name">{target.name}</span>
            {#if target.kind === "group_dm"}<span class="tag">group</span>{/if}
          </label>
        {/each}
      {/if}
    </div>
    {#if tab === "dms"}
      <p class="hint small muted">Only open DMs are listed. Closed conversations can be reopened in Discord first.</p>
    {/if}
  </section>

  <section class="card options" aria-label="What to delete">
    <h2>What to delete</h2>

    <fieldset>
      <legend>Time range</legend>
      <label class="choice">
        <input type="radio" name="range" value="older_than" bind:group={range.mode} />
        <span>Messages older than</span>
      </label>
      <div class="indent inline" class:disabled={range.mode !== "older_than"}>
        <input
          type="number"
          min="1"
          step="1"
          bind:value={range.amount}
          disabled={range.mode !== "older_than"}
          aria-label="Amount"
        />
        <select bind:value={range.unit} disabled={range.mode !== "older_than"} aria-label="Unit">
          <option value="days">days</option>
          <option value="weeks">weeks</option>
          <option value="months">months</option>
          <option value="years">years</option>
        </select>
      </div>

      <label class="choice">
        <input type="radio" name="range" value="between" bind:group={range.mode} />
        <span>Messages sent between</span>
      </label>
      <div class="indent dates" class:disabled={range.mode !== "between"}>
        <input type="date" bind:value={range.from} disabled={range.mode !== "between"} aria-label="From" />
        <span class="muted">and</span>
        <input type="date" bind:value={range.to} disabled={range.mode !== "between"} aria-label="To (inclusive)" />
      </div>

      <label class="choice">
        <input type="radio" name="range" value="all" bind:group={range.mode} />
        <span>All my messages</span>
      </label>
    </fieldset>

    <fieldset>
      <legend>
        Content
        {#if contentActive}
          <button type="button" class="link small" onclick={() => (content = { contains: "", pattern: "", has: [], without: [] })}>
            reset
          </button>
        {/if}
      </legend>
      <label class="field">
        <span class="small muted">Containing all of these words</span>
        <input type="text" placeholder="Any text" bind:value={content.contains} spellcheck="false" />
      </label>
      <div class="field">
        <span class="small muted">Only messages with</span>
        <div class="chips" role="group" aria-label="Only messages with">
          {#each HAS_KINDS as kind (kind.value)}
            <label class="chip" class:on={content.has.includes(kind.value)}>
              <input type="checkbox" checked={content.has.includes(kind.value)} onchange={() => toggleKind("has", kind.value)} />
              {kind.label}
            </label>
          {/each}
        </div>
      </div>
      <div class="field">
        <span class="small muted">Keep messages with</span>
        <div class="chips" role="group" aria-label="Keep messages with">
          {#each HAS_KINDS as kind (kind.value)}
            <label class="chip keep" class:on={content.without.includes(kind.value)}>
              <input
                type="checkbox"
                checked={content.without.includes(kind.value)}
                onchange={() => toggleKind("without", kind.value)}
              />
              {kind.label}
            </label>
          {/each}
        </div>
      </div>
      <details open={!!content.pattern}>
        <summary class="small">Regular expression</summary>
        <label class="field regex">
          <input type="text" placeholder="e.g. ^(lol|ok)$" bind:value={content.pattern} spellcheck="false" aria-label="Regular expression" />
          <span class="small muted">Case-insensitive. Checked by purgecord while deleting, so counts can be too high.</span>
        </label>
      </details>
    </fieldset>

    <fieldset>
      <legend>Options</legend>
      <label class="choice">
        <input type="checkbox" bind:checked={skipPinned} />
        <span>Keep pinned messages</span>
      </label>
    </fieldset>

    <details>
      <summary class="small">Speed</summary>
      <div class="speed small">
        <label>
          <span>Pause after each deletion</span>
          <span class="unit"><input type="number" min="0" step="100" bind:value={options.delete_delay_ms} /> ms</span>
        </label>
        <label>
          <span>Pause between searches</span>
          <span class="unit"><input type="number" min="0" step="500" bind:value={options.search_delay_ms} /> ms</span>
        </label>
        <p class="muted">Shorter pauses are faster but make rate limits, and attention from Discord, more likely.</p>
      </div>
    </details>

    <div class="footer">
      {#if problem}
        <p class="small problem">{problem}</p>
      {:else}
        <p class="small muted">
          {#if selected.size === 0}
            Select at least one server or DM.
          {:else}
            Your messages {summary} in
            {[selectedServers ? plural(selectedServers, "server") : "", selectedDms ? plural(selectedDms, "DM") : ""]
              .filter(Boolean)
              .join(" and ")}.
          {/if}
        </p>
      {/if}
      <button class="btn primary" disabled={selected.size === 0 || !!problem} onclick={onCount}>Count messages</button>
    </div>
  </section>
</div>

<style>
  .setup {
    display: grid;
    grid-template-columns: minmax(280px, 1fr) minmax(300px, 380px);
    gap: 16px;
    height: 100%;
    min-height: 0;
  }

  @media (max-width: 760px) {
    /* Stack the panels and let the page scroll instead of each panel. */
    .setup {
      grid-template-columns: 1fr;
      height: auto;
      flex: none;
    }

    .list {
      max-height: 45vh;
    }

    .options {
      overflow: visible;
    }
  }

  .picker {
    display: flex;
    flex-direction: column;
    min-height: 0;
    overflow: hidden;
  }

  .tabs {
    display: flex;
    border-bottom: 1px solid var(--border);
  }

  .tabs button {
    flex: 1;
    padding: 12px;
    background: none;
    border: none;
    border-bottom: 2px solid transparent;
    font: inherit;
    font-weight: 600;
    color: var(--muted);
    cursor: pointer;
  }

  .tabs button.active {
    color: var(--text);
    border-bottom-color: var(--accent);
  }

  .count {
    font-weight: 500;
    color: var(--muted);
    margin-left: 4px;
  }

  .toolbar {
    display: flex;
    gap: 8px;
    padding: 10px;
    border-bottom: 1px solid var(--border);
  }

  .toolbar input {
    flex: 1;
  }

  .list {
    flex: 1;
    overflow-y: auto;
    padding: 6px;
    min-height: 200px;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 7px 8px;
    border-radius: 8px;
    cursor: pointer;
  }

  .row:hover {
    background: var(--panel-2);
  }

  .row.checked {
    background: var(--accent-soft);
  }

  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .tag {
    font-size: 11px;
    color: var(--muted);
    border: 1px solid var(--border);
    border-radius: 999px;
    padding: 0 7px;
  }

  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 10px;
    padding: 32px 16px;
    text-align: center;
  }

  p.empty {
    flex-direction: row;
  }

  .hint {
    padding: 8px 12px;
    border-top: 1px solid var(--border);
  }

  .options {
    padding: 20px;
    display: flex;
    flex-direction: column;
    gap: 18px;
    overflow-y: auto;
  }

  fieldset {
    border: none;
    padding: 0;
    margin: 0;
    display: grid;
    gap: 8px;
  }

  legend {
    font-weight: 600;
    margin-bottom: 8px;
    padding: 0;
    display: flex;
    align-items: baseline;
    gap: 8px;
  }

  .field {
    display: grid;
    gap: 5px;
  }

  .regex {
    margin-top: 8px;
  }

  .regex input {
    font-family: ui-monospace, "SF Mono", Menlo, Consolas, monospace;
  }

  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .chip {
    display: inline-flex;
    align-items: center;
    padding: 3px 10px;
    border: 1px solid var(--border);
    border-radius: 999px;
    font-size: 13px;
    cursor: pointer;
    user-select: none;
  }

  .chip input {
    position: absolute;
    opacity: 0;
    pointer-events: none;
  }

  .chip:has(input:focus-visible) {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }

  .chip.on {
    background: var(--accent-soft);
    border-color: var(--accent);
  }

  .chip.keep.on {
    background: var(--ok-soft);
    border-color: var(--ok);
  }

  .link {
    background: none;
    border: none;
    padding: 0;
    font: inherit;
    font-weight: 400;
    color: var(--accent);
    cursor: pointer;
  }

  .choice {
    display: flex;
    align-items: center;
    gap: 8px;
    cursor: pointer;
  }

  .indent {
    padding-left: 24px;
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }

  .indent.disabled {
    opacity: 0.55;
  }

  .inline input {
    width: 80px;
  }

  .dates input {
    flex: 1;
    min-width: 130px;
  }

  details summary {
    cursor: pointer;
    font-weight: 600;
  }

  .speed {
    display: grid;
    gap: 8px;
    margin-top: 10px;
  }

  .speed label {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 8px;
  }

  .unit {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--muted);
  }

  .unit input {
    width: 90px;
  }

  .footer {
    margin-top: auto;
    display: grid;
    gap: 10px;
    padding-top: 14px;
    border-top: 1px solid var(--border);
  }

  .problem {
    color: var(--danger);
  }
</style>
