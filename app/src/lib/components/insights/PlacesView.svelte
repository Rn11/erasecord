<script lang="ts">
  import { api } from "$lib/api";
  import { num, t } from "$lib/i18n.svelte";
  import { formatDate, percent } from "$lib/insights/chart";
  import { useQuery } from "$lib/insights/query.svelte";
  import { insights, placeName } from "$lib/insights/store.svelte";
  import { hideTip, showTip } from "$lib/insights/tooltip.svelte";
  import type { CleanUpRequest, Scope } from "$lib/insights/types";
  import type { PackageSummary } from "$lib/types";
  import Avatar from "../Avatar.svelte";
  import Sparkline from "./Sparkline.svelte";

  let {
    scope = $bindable(),
    pkg,
    onCleanUp,
  }: { scope: Scope; pkg: PackageSummary | null; onCleanUp: (request: CleanUpRequest) => void } = $props();

  const q = useQuery("places", () => scope, api.insightsPlaces);
  const data = $derived(q.data);
  let showAll = $state(false);
  const total = $derived((data?.places ?? []).reduce((sum, p) => sum + p.messages, 0));
  const max = $derived(Math.max(1, ...(data?.places ?? []).map((p) => p.messages)));
  const rows = $derived(showAll ? (data?.places ?? []) : (data?.places ?? []).slice(0, 25));
  const onlyOneServer = $derived(
    scope.places.length === 1 && insights.info?.places.find((p) => p.id === scope.places[0])?.kind === "guild",
  );
  const channelMax = $derived(Math.max(1, ...(data?.channels ?? []).map((c) => c.messages)));

  const kindLabel = (index: number) => {
    const kind = insights.info?.places[index]?.kind;
    return kind === "guild" ? t("insights.kindServer") : kind === "group_dm" ? t("setup.tagGroup") : "DM";
  };

  function focus(index: number) {
    const id = insights.info?.places[index]?.id;
    if (!id) return;
    scope.channels = [];
    scope.places = [id];
  }

  function cleanUp(index: number, channel?: string) {
    const id = insights.info?.places[index]?.id;
    if (!id) return;
    onCleanUp({ places: [id], channels: channel ? [channel] : [], from: scope.from, to: scope.to, contains: "" });
  }
</script>

{#if q.error}
  <p class="callout error small">{q.error}</p>
{:else if !data}
  <p class="muted"><span class="spinner"></span> {t("stats.loading")}</p>
{:else if data.places.length === 0}
  <p class="muted">{t("insights.nothingHere")}</p>
{:else}
  <div class="places" class:stale={q.loading}>
    {#if onlyOneServer && data.channels.length > 0}
      <figure>
        <figcaption>
          <span>{t("insights.channelsOf", { name: placeName(data.places[0].place, pkg) })}</span>
          <span class="muted small">{t("insights.clickChannel")}</span>
        </figcaption>
        <div class="list">
          {#each data.channels as channel (channel.id)}
            <div class="row channel" class:chosen={scope.channels.includes(channel.id)}>
              <button class="main" onclick={() => (scope.channels = scope.channels.includes(channel.id) ? [] : [channel.id])}>
                <span class="name">#{channel.name}</span>
                <span class="track">
                  <span class="bar" style:width="{(channel.messages / channelMax) * 100}%"></span>
                  <span class="value num">{num(channel.messages)}</span>
                </span>
              </button>
              <button class="link small" onclick={() => cleanUp(channel.place, channel.id)}>{t("insights.cleanUp")}</button>
            </div>
          {/each}
        </div>
      </figure>
    {/if}

    <figure>
      <figcaption>
        <span>{t("insights.ranking")}</span>
        <span class="muted small">{t("insights.clickPlace")}</span>
      </figcaption>
      <div class="list" role="table">
        <div class="row head small muted" role="row">
          <span role="columnheader">{t("insights.place")}</span>
          <span role="columnheader">{t("stats.messages")}</span>
          <span class="extra" role="columnheader">{t("insights.activity")}</span>
          <span class="extra dates" role="columnheader">{t("insights.firstLast")}</span>
          <span></span>
        </div>
        {#each rows as row (row.place)}
          <div class="row" role="row">
            <button
              class="main"
              onclick={() => focus(row.place)}
              onpointermove={(e) =>
                showTip(e, placeName(row.place, pkg), [
                  { label: t("stats.messages"), value: num(row.messages) },
                  { label: t("insights.share"), value: percent(row.messages / Math.max(1, total)) },
                  { label: t("stats.words"), value: num(row.words) },
                  { label: t("stats.attachments"), value: num(row.attachments) },
                  { label: t("stats.activeDays"), value: num(row.active_days) },
                ])}
              onpointerleave={hideTip}
            >
              <span class="name">
                <Avatar name={placeName(row.place, pkg)} url={null} size={22} />
                <span class="label">{placeName(row.place, pkg)}</span>
                <span class="kind small muted">{kindLabel(row.place)}</span>
              </span>
              <span class="track">
                <span class="bar" style:width="{(row.messages / max) * 100}%"></span>
                <span class="value num">{num(row.messages)}</span>
              </span>
            </button>
            <span class="extra"><Sparkline values={row.monthly} /></span>
            <span class="extra dates small muted">{formatDate(row.first)} – {formatDate(row.last)}</span>
            <button class="link small" onclick={() => cleanUp(row.place)}>{t("insights.cleanUp")}</button>
          </div>
        {/each}
      </div>
      {#if data.places.length > rows.length || showAll}
        <button class="link small more" onclick={() => (showAll = !showAll)}>
          {showAll ? t("insights.showFewer") : t("insights.showAll", { count: data.places.length })}
        </button>
      {/if}
      <p class="muted small">{t("insights.sparklineNote")}</p>
    </figure>
  </div>
{/if}

<style>
  .places {
    display: grid;
    gap: 24px;
    transition: opacity 0.15s;
  }

  .stale {
    opacity: 0.6;
  }

  figure {
    margin: 0;
    display: grid;
    gap: 8px;
  }

  figcaption {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 12px;
    font-weight: 600;
  }

  figure p {
    margin: 0;
  }

  .list {
    display: grid;
  }

  .row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 130px 190px auto;
    gap: 14px;
    align-items: center;
    padding: 3px 6px;
    border-radius: 6px;
  }

  .row.channel {
    grid-template-columns: minmax(0, 1fr) auto;
  }

  .row:not(.head):hover,
  .row.chosen {
    background: var(--panel-2);
  }

  .row.head {
    padding-bottom: 6px;
    border-bottom: 1px solid var(--border);
    margin-bottom: 4px;
  }

  .row.head > :first-child {
    display: grid;
    grid-template-columns: minmax(120px, 280px) 1fr;
  }

  .main {
    display: grid;
    grid-template-columns: minmax(120px, 280px) minmax(0, 1fr);
    gap: 12px;
    align-items: center;
    border: none;
    background: none;
    font: inherit;
    color: inherit;
    text-align: left;
    padding: 0;
    cursor: pointer;
    min-width: 0;
  }

  .name {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }

  .label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .kind {
    flex: none;
  }

  .track {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }

  .bar {
    height: 14px;
    min-width: 2px;
    max-width: calc(100% - 64px);
    background: var(--accent);
    border-radius: 0 4px 4px 0;
  }

  .value {
    font-size: 13px;
  }

  .dates {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .link {
    background: none;
    border: none;
    padding: 0;
    font: inherit;
    color: var(--accent);
    cursor: pointer;
    white-space: nowrap;
  }

  .more {
    justify-self: start;
  }

  @media (max-width: 860px) {
    .row {
      grid-template-columns: minmax(0, 1fr) auto;
    }
    .extra {
      display: none;
    }
    .main {
      grid-template-columns: minmax(90px, 160px) minmax(0, 1fr);
    }
  }
</style>
