<script lang="ts">
  import { api } from "$lib/api";
  import { num, t } from "$lib/i18n.svelte";
  import { formatDate } from "$lib/insights/chart";
  import { useQuery } from "$lib/insights/query.svelte";
  import { insights, placeName } from "$lib/insights/store.svelte";
  import type { CleanUpRequest, Scope } from "$lib/insights/types";
  import type { PackageSummary } from "$lib/types";

  let {
    scope = $bindable(),
    pkg,
    onCleanUp,
  }: { scope: Scope; pkg: PackageSummary | null; onCleanUp: (request: CleanUpRequest) => void } = $props();

  const LIMIT = 100;
  let draft = $state(insights.query);
  $effect(() => {
    draft = insights.query;
  });
  const words = $derived(insights.query.trim().toLowerCase().split(/\s+/).filter(Boolean));
  const q = useQuery(
    "search",
    () => scope,
    (s) => (words.length ? api.insightsSearch(s, insights.query, LIMIT) : Promise.resolve(null)),
    () => insights.query,
  );
  const data = $derived(q.data);

  function submit(event: SubmitEvent) {
    event.preventDefault();
    insights.query = draft.trim();
  }

  /** The text in pieces, with the searched words marked. */
  function pieces(text: string): { text: string; mark: boolean }[] {
    if (words.length === 0) return [{ text, mark: false }];
    const lower = text.toLowerCase();
    const marks = new Array(text.length).fill(false);
    for (const word of words) {
      for (let at = lower.indexOf(word); at !== -1; at = lower.indexOf(word, at + word.length)) {
        for (let i = at; i < at + word.length; i++) marks[i] = true;
      }
    }
    const out: { text: string; mark: boolean }[] = [];
    for (let i = 0; i < text.length; i++) {
      const last = out[out.length - 1];
      if (last && last.mark === marks[i]) last.text += text[i];
      else out.push({ text: text[i], mark: marks[i] });
    }
    return out;
  }

  function cleanUp() {
    if (!data) return;
    const places = scope.places.length > 0 ? scope.places : data.places.map(([p]) => insights.info?.places[p]?.id ?? "").filter(Boolean);
    onCleanUp({ places, channels: scope.channels, from: scope.from, to: scope.to, contains: insights.query });
  }
</script>

<div class="search">
  <form class="bar" onsubmit={submit}>
    <input type="search" placeholder={t("insights.searchPlaceholder")} bind:value={draft} aria-label={t("insights.search")} />
    <button class="btn primary" type="submit" disabled={!draft.trim()}>{t("insights.search")}</button>
  </form>
  <p class="muted small">{t("insights.searchHint")}</p>

  {#if q.error}
    <p class="callout error small">{q.error}</p>
  {:else if words.length === 0}
    <!-- Nothing asked yet. -->
  {:else if !data || (q.loading && data === null)}
    <p class="muted"><span class="spinner"></span> {t("insights.searching")}</p>
  {:else}
    <div class="results" class:stale={q.loading}>
      <div class="summary">
        <strong>{t("insights.found", { count: data.total })}</strong>
        {#if data.total > 0}
          <button class="btn small" onclick={cleanUp}>{t("insights.cleanUpFound", { count: data.total })}</button>
        {/if}
      </div>
      {#if data.places.length > 1}
        <div class="places small">
          {#each data.places.slice(0, 10) as [place, count] (place)}
            <button class="chip" onclick={() => (scope.places = [insights.info?.places[place]?.id ?? ""])}>
              {placeName(place, pkg)} <span class="muted num">{num(count)}</span>
            </button>
          {/each}
        </div>
      {/if}
      <ol class="messages">
        {#each data.messages as m (m.id)}
          <li>
            <span class="meta muted small">
              {formatDate(m.sent_at, { dateStyle: "medium", timeStyle: "short" })} · {placeName(m.place, pkg)}{m.channel &&
              m.channel !== placeName(m.place, pkg)
                ? ` · #${m.channel}`
                : ""}
            </span>
            <span class="text">
              {#each pieces(m.text) as piece, i (i)}{#if piece.mark}<mark>{piece.text}</mark>{:else}{piece.text}{/if}{/each}
            </span>
          </li>
        {/each}
      </ol>
      {#if data.total > data.messages.length}
        <p class="muted small">{t("insights.newestShown", { count: data.messages.length })}</p>
      {/if}
      {#if data.total > 0}<p class="muted small">{t("insights.cleanUpHint")}</p>{/if}
    </div>
  {/if}
</div>

<style>
  .search {
    display: grid;
    gap: 10px;
  }

  .bar {
    display: flex;
    gap: 8px;
  }

  .bar input {
    flex: 1;
  }

  .search > p {
    margin: 0;
  }

  .results {
    display: grid;
    gap: 12px;
    margin-top: 6px;
    transition: opacity 0.15s;
  }

  .stale {
    opacity: 0.6;
  }

  .results p {
    margin: 0;
  }

  .summary {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    flex-wrap: wrap;
  }

  .places {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .chip {
    border: 1px solid var(--border);
    border-radius: 999px;
    background: none;
    font: inherit;
    color: inherit;
    padding: 3px 10px;
    cursor: pointer;
  }

  .chip:hover {
    background: var(--panel-2);
  }

  .messages {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    border: 1px solid var(--border);
    border-radius: 10px;
  }

  .messages li {
    display: grid;
    gap: 2px;
    padding: 8px 12px;
    border-bottom: 1px solid var(--border);
  }

  .messages li:last-child {
    border-bottom: none;
  }

  .text {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  mark {
    background: var(--warn-soft);
    color: inherit;
    border-radius: 3px;
    padding: 0 1px;
  }
</style>
