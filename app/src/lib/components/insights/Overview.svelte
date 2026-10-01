<script lang="ts">
  import { api } from "$lib/api";
  import { num, t } from "$lib/i18n.svelte";
  import { compact, formatDate } from "$lib/insights/chart";
  import { useQuery } from "$lib/insights/query.svelte";
  import { save } from "@tauri-apps/plugin-dialog";
  import { errorMessage } from "$lib/errors";
  import { ask, insights, placeName } from "$lib/insights/store.svelte";
  import { drawYearCard, pngBytes } from "$lib/insights/yearCard";
  import { hideTip, showTip } from "$lib/insights/tooltip.svelte";
  import type { MessageRef, Scope, Span } from "$lib/insights/types";
  import type { PackageSummary } from "$lib/types";

  let { scope = $bindable(), pkg }: { scope: Scope; pkg: PackageSummary | null } = $props();

  const q = useQuery("overview", () => scope, api.insightsOverview);
  const o = $derived(q.data);
  const maxYear = $derived(Math.max(1, ...(o?.years ?? []).map((y) => y.messages)));
  const span = (s: Span) => t("insights.spanDates", { from: formatDate(s.from), to: formatDate(s.to) });

  /** The year when the period is exactly one calendar year. */
  const year = $derived.by(() => {
    const y = scope.from?.slice(0, 4);
    return y && scope.from === `${y}-01-01` && scope.to === `${y}-12-31` ? Number(y) : null;
  });
  let cardBusy = $state(false);
  let cardResult = $state<{ path?: string; error?: string } | null>(null);

  async function saveYearCard() {
    if (!o || year === null) return;
    cardResult = null;
    try {
      const path = await save({
        title: t("insights.yearCardSave", { year }),
        defaultPath: `erasecord-${year}.png`,
        filters: [{ name: "PNG", extensions: ["png"] }],
      });
      if (!path) return;
      cardBusy = true;
      const [places, words] = await Promise.all([
        ask("places", scope, api.insightsPlaces),
        ask("words", scope, api.insightsWords),
      ]);
      // Servers only: DM names are other people's.
      const servers = places.places
        .filter((p) => insights.info?.places[p.place]?.kind === "guild")
        .slice(0, 3)
        .map((p) => placeName(p.place, pkg));
      const canvas = drawYearCard({
        title: t("insights.yearCardTitle", { year }),
        figures: [
          [t("stats.messages"), num(o.messages)],
          [t("stats.words"), num(o.words)],
          [t("stats.activeDays"), num(o.active_days)],
          ...(o.longest_streak
            ? [[t("insights.streak"), t("insights.days", { count: o.longest_streak.days })] as [string, string]]
            : []),
          ...(o.busiest_day ? [[t("stats.busiestDay"), formatDate(o.busiest_day.date)] as [string, string]] : []),
          [t("stats.attachments"), num(o.attachments)],
        ],
        serversLabel: t("insights.yearCardServers"),
        servers,
        emojiLabel: t("insights.yearCardEmoji"),
        emoji: words.emoji
          .filter((e) => e.id === null)
          .slice(0, 5)
          .map((e) => e.emoji),
        footer: "EraseCord",
      });
      await api.savePng(path, await pngBytes(canvas));
      cardResult = { path };
    } catch (err) {
      cardResult = { error: errorMessage(err) };
    } finally {
      cardBusy = false;
    }
  }
</script>

{#snippet message(title: string, m: MessageRef)}
  <figure class="message">
    <figcaption>
      <span class="label">{title}</span>
      <span class="muted small">
        {formatDate(m.sent_at, { dateStyle: "long", timeStyle: "short" })} · {placeName(m.place, pkg)}{m.channel &&
        m.channel !== placeName(m.place, pkg)
          ? ` · #${m.channel}`
          : ""}
      </span>
    </figcaption>
    <blockquote>
      {#if m.text}{m.text}{:else}<span class="muted">{t("preview.noPreview")}</span>{/if}
      {#if m.attachments > 0}<span class="muted small"> {t("preview.attachments", { count: m.attachments })}</span>{/if}
    </blockquote>
  </figure>
{/snippet}

{#if q.error}
  <p class="callout error small">{q.error}</p>
{:else if !o}
  <p class="muted"><span class="spinner"></span> {t("stats.loading")}</p>
{:else if o.messages === 0}
  <p class="muted empty">{t("insights.nothingHere")}</p>
{:else}
  <div class="overview" class:stale={q.loading}>
    {#if year !== null}
      <div class="year-card small">
        <button class="btn small" onclick={saveYearCard} disabled={cardBusy}>
          {#if cardBusy}<span class="spinner"></span>{/if}
          {t("insights.yearCardSave", { year })}
        </button>
        <span class="muted">{t("insights.yearCardHint")}</span>
        {#if cardResult?.path}<span>{t("insights.yearCardSaved", { path: cardResult.path })}</span>{/if}
        {#if cardResult?.error}<span class="callout error">{cardResult.error}</span>{/if}
      </div>
    {/if}
    <div class="figures">
      <div class="hero">
        <span class="label">{t("stats.messages")}</span>
        <span class="value">{num(o.messages)}</span>
        <span class="detail">
          {t("insights.onDays", { active: num(o.active_days), span: num(o.span_days) })}
        </span>
      </div>
      <div class="tiles">
        <div class="tile">
          <span class="label">{t("stats.words")}</span>
          <span class="value" title={num(o.words)}>{compact(o.words)}</span>
          <span class="detail">
            {t("insights.perMessage", { count: Math.round(o.words / Math.max(1, o.messages - o.without_text)) })}
          </span>
        </div>
        <div class="tile">
          <span class="label">{t("stats.attachments")}</span>
          <span class="value">{num(o.attachments)}</span>
          <span class="detail">{t("stats.inMessages", { count: o.with_attachments })}</span>
        </div>
        <div class="tile">
          <span class="label">{t("insights.links")}</span>
          <span class="value">{num(o.links)}</span>
          <span class="detail">{t("stats.withoutText", { count: o.without_text })}</span>
        </div>
        <div class="tile">
          <span class="label">{t("insights.placesUsed")}</span>
          <span class="value">{num(o.places)}</span>
          <span class="detail">{t("stats.perDay", { count: Math.round(o.messages / Math.max(1, o.active_days)) })}</span>
        </div>
        {#if o.busiest_day}
          <div class="tile">
            <span class="label">{t("stats.busiestDay")}</span>
            <span class="value">{formatDate(o.busiest_day.date)}</span>
            <span class="detail">{t("count.message", { count: o.busiest_day.messages })}</span>
          </div>
        {/if}
        {#if o.longest_streak}
          <div class="tile">
            <span class="label">{t("insights.streak")}</span>
            <span class="value">{t("insights.days", { count: o.longest_streak.days })}</span>
            <span class="detail">{span(o.longest_streak)}</span>
          </div>
        {/if}
        {#if o.longest_break}
          <div class="tile">
            <span class="label">{t("insights.break")}</span>
            <span class="value">{t("insights.days", { count: o.longest_break.days })}</span>
            <span class="detail">{span(o.longest_break)}</span>
          </div>
        {/if}
      </div>
    </div>

    <div class="messages">
      {#if o.first}{@render message(t("insights.firstMessage"), o.first)}{/if}
      {#if o.last && o.last.id !== o.first?.id}{@render message(t("insights.lastMessage"), o.last)}{/if}
    </div>

    {#if o.years.length > 1}
      <figure>
        <figcaption class="caption">
          <span>{t("insights.perYear")}</span>
          <span class="muted small">{t("insights.clickYear")}</span>
        </figcaption>
        <div class="years" role="table">
          {#each o.years as year (year.year)}
            <button
              class="year"
              role="row"
              onclick={() => Object.assign(scope, { from: `${year.year}-01-01`, to: `${year.year}-12-31` })}
              onpointermove={(e) =>
                showTip(e, String(year.year), [
                  { label: t("stats.messages"), value: num(year.messages) },
                  { label: t("stats.words"), value: num(year.words) },
                  { label: t("stats.activeDays"), value: num(year.active_days) },
                ])}
              onpointerleave={hideTip}
            >
              <span class="year-label num" role="rowheader">{year.year}</span>
              <span class="track" role="cell">
                <span class="bar" style:width="{(year.messages / maxYear) * 100}%"></span>
                <span class="bar-value num">{num(year.messages)}</span>
              </span>
              <span class="top muted" role="cell">
                {#if year.top_place !== null}{t("insights.mostIn", { name: placeName(year.top_place, pkg) })}{/if}
              </span>
            </button>
          {/each}
        </div>
      </figure>
    {/if}
  </div>
{/if}

<style>
  .overview {
    display: grid;
    gap: 22px;
    transition: opacity 0.15s;
  }

  .stale {
    opacity: 0.6;
  }

  .empty {
    padding: 24px 0;
  }

  .figures {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 24px;
    align-items: center;
  }

  .hero,
  .tile {
    display: grid;
    gap: 2px;
    align-content: start;
  }

  .label {
    font-size: 13px;
    color: var(--muted);
  }

  .hero .value {
    font-size: 52px;
    font-weight: 650;
    line-height: 1.05;
  }

  .tiles {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
    gap: 10px;
  }

  .tile {
    padding: 10px 12px;
    border: 1px solid var(--border);
    border-radius: 10px;
  }

  .tile .value {
    font-size: 19px;
    font-weight: 600;
  }

  .detail {
    font-size: 12px;
    color: var(--muted);
  }

  .messages {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(260px, 1fr));
    gap: 12px;
  }

  figure {
    margin: 0;
    display: grid;
    gap: 8px;
  }

  .message {
    padding: 12px 14px;
    border: 1px solid var(--border);
    border-radius: 10px;
    align-content: start;
  }

  .message figcaption {
    display: grid;
    gap: 2px;
  }

  blockquote {
    margin: 0;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    max-height: 7.5em;
    overflow: hidden;
  }

  .caption {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    font-weight: 600;
  }

  .year-card {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem 0.75rem;
  }
  .years {
    display: grid;
    gap: 2px;
  }

  .year {
    display: grid;
    grid-template-columns: 48px minmax(0, 1fr) minmax(0, 220px);
    gap: 12px;
    align-items: center;
    padding: 4px 6px;
    border: none;
    border-radius: 6px;
    background: none;
    font: inherit;
    color: inherit;
    text-align: left;
    cursor: pointer;
  }

  .year:hover {
    background: var(--panel-2);
  }

  .track {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }

  .bar {
    height: 16px;
    min-width: 2px;
    max-width: calc(100% - 64px);
    background: var(--accent);
    border-radius: 0 4px 4px 0;
  }

  .bar-value {
    font-size: 13px;
  }

  .top {
    font-size: 13px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  @media (max-width: 720px) {
    .figures {
      grid-template-columns: 1fr;
    }
    .year {
      grid-template-columns: 44px minmax(0, 1fr);
    }
    .top {
      display: none;
    }
  }
</style>
