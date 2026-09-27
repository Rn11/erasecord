<script lang="ts">
  import { onMount } from "svelte";
  import { api } from "$lib/api";
  import { errorMessage } from "$lib/errors";
  import { i18n, num, t } from "$lib/i18n.svelte";
  import type { PackageSummary, Statistics } from "$lib/types";
  import Avatar from "./Avatar.svelte";

  let { pkg, onBack }: { pkg: PackageSummary; onBack: () => void } = $props();

  let stats = $state<Statistics | null>(null);
  let error = $state<string | null>(null);

  async function load() {
    error = null;
    try {
      // getTimezoneOffset is minutes west of UTC.
      stats = await api.packageStats(-new Date().getTimezoneOffset());
    } catch (err) {
      error = errorMessage(err);
    }
  }
  onMount(load);

  // Formatting
  const date = (value: Date, options: Intl.DateTimeFormatOptions) =>
    new Intl.DateTimeFormat(i18n.locale, options).format(value);
  const day = (iso: string) => {
    const [y, m, d] = iso.split("-").map(Number);
    return new Date(y, m - 1, d);
  };
  const month = (key: string) => {
    const [y, m] = key.split("-").map(Number);
    return new Date(y, m - 1, 1);
  };
  const compact = (value: number) =>
    new Intl.NumberFormat(i18n.locale, { notation: "compact", maximumFractionDigits: 1 }).format(value);

  // Tooltip, shared by all charts.
  let root = $state<HTMLElement>();
  let tip = $state<{ x: number; y: number; title: string; value: string } | null>(null);
  function show(event: PointerEvent | FocusEvent, title: string, value: string) {
    if (!root) return;
    const box = root.getBoundingClientRect();
    const target = (event.currentTarget as Element).getBoundingClientRect();
    const x = "clientX" in event ? event.clientX : target.left + target.width / 2;
    const y = "clientY" in event ? event.clientY : target.top;
    tip = { x: x - box.left + root.scrollLeft, y: y - box.top + root.scrollTop, title, value };
  }
  const hide = () => (tip = null);

  // Messages per month: columns from one baseline, clean ticks.
  let chartWidth = $state(640);
  const HEIGHT = 190;
  const PAD = { top: 10, right: 4, bottom: 24, left: 44 };
  let showTable = $state(false);
  let hoverMonth = $state<string | null>(null);

  function niceStep(max: number) {
    const raw = Math.max(max, 1) / 4;
    const power = 10 ** Math.floor(Math.log10(raw));
    const step = [1, 2, 2.5, 5, 10].map((f) => f * power).find((s) => s >= raw) ?? raw;
    return Math.max(1, step);
  }

  const monthChart = $derived.by(() => {
    const months = stats?.months ?? [];
    const max = Math.max(1, ...months.map((m) => m.messages));
    const step = niceStep(max);
    const top = Math.ceil(max / step) * step;
    const plotW = Math.max(1, chartWidth - PAD.left - PAD.right);
    const plotH = HEIGHT - PAD.top - PAD.bottom;
    const slot = plotW / Math.max(1, months.length);
    const barW = Math.max(1, Math.min(24, slot - 2));
    const y = (v: number) => PAD.top + plotH - (v / top) * plotH;
    const ticks = Array.from({ length: Math.round(top / step) + 1 }, (_, i) => i * step);
    // Label Januaries (years); short spans get every month that fits.
    const everyMonth = months.length <= 14 && slot >= 30;
    // The first month gets a year label too, if there is room before the
    // next January.
    const firstJanuary = months.findIndex((m) => m.month.endsWith("-01"));
    const roomForFirst = firstJanuary === -1 || firstJanuary * slot >= 36;
    const labels = months
      .map((m, i) => ({ i, m }))
      .filter(({ i, m }) => everyMonth || m.month.endsWith("-01") || (i === 0 && roomForFirst))
      .map(({ i, m }) => ({
        x: PAD.left + i * slot + slot / 2,
        text: everyMonth ? date(month(m.month), { month: "short" }) : m.month.slice(0, 4),
        anchor: everyMonth ? "middle" : "start",
      }));
    const bars = months.map((m, i) => {
      const x = PAD.left + i * slot + (slot - barW) / 2;
      const h = PAD.top + plotH - y(m.messages);
      const r = Math.min(4, barW / 2, h);
      // Rounded at the data end, square at the baseline.
      const path =
        h <= 0
          ? ""
          : `M${x},${PAD.top + plotH}V${y(m.messages) + r}q0,-${r} ${r},-${r}h${barW - 2 * r}q${r},0 ${r},${r}V${PAD.top + plotH}Z`;
      return { ...m, path, slotX: PAD.left + i * slot, slot };
    });
    return { bars, ticks, y, labels, plotH };
  });

  // Weekday × hour: one hue, light to dark, five steps.
  const weekdays = $derived(
    Array.from({ length: 7 }, (_, i) => ({
      short: date(new Date(2024, 0, 1 + i), { weekday: "short" }),
      long: date(new Date(2024, 0, 1 + i), { weekday: "long" }),
    })),
  );
  const hour = (h: number) => date(new Date(2024, 0, 1, h), { hour: "numeric" });
  const weekMax = $derived(Math.max(1, ...(stats?.week ?? []).flat()));
  const level = (value: number) => (value === 0 ? 0 : Math.max(1, Math.ceil((value / weekMax) * 5)));

  // Where: the busiest places, the rest folded together.
  const TOP = 8;
  const places = $derived.by(() => {
    const sorted = [...pkg.targets].sort((a, b) => b.messages - a.messages);
    const rest = sorted.slice(TOP);
    return {
      top: sorted.slice(0, TOP),
      others: rest.length,
      othersMessages: rest.reduce((sum, p) => sum + p.messages, 0),
      max: Math.max(1, ...sorted.map((p) => p.messages)),
    };
  });
  const share = (value: number) =>
    new Intl.NumberFormat(i18n.locale, { style: "percent", maximumFractionDigits: 1 }).format(
      value / Math.max(1, pkg.messages),
    );
</script>

<section class="card stats" bind:this={root} onscroll={hide}>
  <header>
    <div>
      <h2>{t("stats.title")}</h2>
      {#if stats?.first_message && stats.last_message}
        <p class="muted small">
          {t("stats.span", {
            first: date(new Date(stats.first_message), { dateStyle: "medium" }),
            last: date(new Date(stats.last_message), { dateStyle: "medium" }),
          })}
        </p>
      {/if}
    </div>
    <button class="btn ghost small" onclick={onBack}>{t("common.back")}</button>
  </header>

  {#if error}
    <p class="callout error small">{error}</p>
    <button class="btn small" onclick={load}>{t("common.tryAgain")}</button>
  {:else if !stats}
    <p class="muted"><span class="spinner"></span> {t("stats.loading")}</p>
  {:else}
    <div class="figures">
      <div class="hero">
        <span class="label">{t("stats.messages")}</span>
        <span class="value">{num(stats.messages)}</span>
      </div>
      <div class="tiles">
        <div class="tile">
          <span class="label">{t("stats.activeDays")}</span>
          <span class="value">{num(stats.active_days)}</span>
          <span class="detail">
            {t("stats.perDay", { count: Math.round(stats.messages / Math.max(1, stats.active_days)) })}
          </span>
        </div>
        <div class="tile">
          <span class="label">{t("stats.words")}</span>
          <span class="value" title={num(stats.words)}>{compact(stats.words)}</span>
          <span class="detail">{t("stats.withoutText", { count: stats.without_text })}</span>
        </div>
        <div class="tile">
          <span class="label">{t("stats.attachments")}</span>
          <span class="value">{num(stats.attachments)}</span>
          <span class="detail">{t("stats.inMessages", { count: stats.with_attachments })}</span>
        </div>
        {#if stats.busiest_day}
          <div class="tile">
            <span class="label">{t("stats.busiestDay")}</span>
            <span class="value">{date(day(stats.busiest_day.date), { dateStyle: "medium" })}</span>
            <span class="detail">{t("count.message", { count: stats.busiest_day.messages })}</span>
          </div>
        {/if}
      </div>
    </div>

    <figure>
      <figcaption>
        <span>{t("stats.perMonth")}</span>
        <button class="link small" onclick={() => (showTable = !showTable)}>
          {showTable ? t("stats.hideTable") : t("stats.showTable")}
        </button>
      </figcaption>
      {#if showTable}
        <div class="table-view">
          <table class="small">
            <thead><tr><th>{t("stats.month")}</th><th class="num">{t("stats.messages")}</th></tr></thead>
            <tbody>
              {#each [...stats.months].reverse() as m (m.month)}
                <tr><td>{date(month(m.month), { month: "long", year: "numeric" })}</td><td class="num">{num(m.messages)}</td></tr>
              {/each}
            </tbody>
          </table>
        </div>
      {:else}
        <div class="chart" bind:clientWidth={chartWidth}>
          <svg width={chartWidth} height={HEIGHT} role="img" aria-label={t("stats.perMonth")}>
            {#each monthChart.ticks as tick (tick)}
              <line class="grid" x1={PAD.left} x2={chartWidth - PAD.right} y1={monthChart.y(tick)} y2={monthChart.y(tick)} />
              <text class="axis" x={PAD.left - 8} y={monthChart.y(tick)} dy="0.32em" text-anchor="end">{compact(tick)}</text>
            {/each}
            {#each monthChart.bars as bar (bar.month)}
              {#if hoverMonth === bar.month}
                <rect class="hover-band" x={bar.slotX} y={PAD.top} width={bar.slot} height={monthChart.plotH} />
              {/if}
              <path class="bar" d={bar.path} />
            {/each}
            {#each monthChart.labels as label (label.x)}
              <text class="axis" x={label.x} y={HEIGHT - 6} text-anchor={label.anchor}>{label.text}</text>
            {/each}
            {#each monthChart.bars as bar (bar.month)}
              <rect
                class="hit"
                role="presentation"
                x={bar.slotX}
                y={PAD.top}
                width={bar.slot}
                height={monthChart.plotH}
                onpointermove={(e) => {
                  hoverMonth = bar.month;
                  show(e, date(month(bar.month), { month: "long", year: "numeric" }), t("count.message", { count: bar.messages }));
                }}
                onpointerleave={() => {
                  hoverMonth = null;
                  hide();
                }}
              />
            {/each}
          </svg>
        </div>
      {/if}
    </figure>

    <figure>
      <figcaption><span>{t("stats.week")}</span></figcaption>
      <div class="heatmap" role="table" aria-label={t("stats.week")}>
        {#each stats.week as hours, d (d)}
          <div class="weekday small muted" role="rowheader">{weekdays[d].short}</div>
          {#each hours as value, h (h)}
            <div
              class="cell level-{level(value)}"
              role="cell"
              tabindex="-1"
              aria-label="{weekdays[d].long} {hour(h)}: {t('count.message', { count: value })}"
              onpointermove={(e) =>
                show(e, t("stats.hourRange", { day: weekdays[d].long, from: hour(h), to: hour((h + 1) % 24) }), t("count.message", { count: value }))}
              onpointerleave={hide}
            ></div>
          {/each}
        {/each}
        <div></div>
        {#each Array.from({ length: 24 }, (_, h) => h) as h (h)}
          <div class="hour small muted">{h % 6 === 0 ? hour(h) : ""}</div>
        {/each}
      </div>
      <div class="legend small muted">
        <span>{t("stats.fewer")}</span>
        {#each [0, 1, 2, 3, 4, 5] as l (l)}<span class="swatch level-{l}"></span>{/each}
        <span>{t("stats.more")}</span>
        <span class="zone">{t("stats.timeZone")}</span>
      </div>
    </figure>

    <figure>
      <figcaption><span>{t("stats.places")}</span></figcaption>
      <div class="places">
        {#each places.top as place (place.target.id)}
          <Avatar name={place.target.name} url={place.target.icon_url} size={20} />
          <span class="place-name small" title={place.target.name}>{place.target.name}</span>
          <span class="track">
            <span
              class="hbar"
              style:width="{(place.messages / places.max) * 100}%"
              role="presentation"
              onpointermove={(e) => show(e, place.target.name, `${t("count.message", { count: place.messages })} · ${share(place.messages)}`)}
              onpointerleave={hide}
            ></span>
            <span class="tip-value small num">{num(place.messages)}</span>
          </span>
        {/each}
        {#if places.others > 0}
          <span></span>
          <span class="place-name small muted">{t("stats.others", { count: places.others })}</span>
          <span class="track">
            <span class="tip-value small num muted">{num(places.othersMessages)}</span>
          </span>
        {/if}
      </div>
    </figure>
  {/if}

  {#if tip}
    <div class="tooltip small" style:left="{tip.x}px" style:top="{tip.y}px" role="status">
      <strong>{tip.title}</strong>
      <span>{tip.value}</span>
    </div>
  {/if}
</section>

<style>
  .stats {
    position: relative;
    width: 100%;
    max-width: 880px;
    max-height: 100%;
    margin: 0 auto;
    padding: 20px;
    display: grid;
    gap: 22px;
    overflow-y: auto;
    /* Sequential steps of the accent, light to dark; step 0 is "none". */
    --seq-0: color-mix(in oklab, var(--border) 45%, var(--panel));
    --seq-1: color-mix(in oklab, var(--accent) 18%, var(--panel));
    --seq-2: color-mix(in oklab, var(--accent) 36%, var(--panel));
    --seq-3: color-mix(in oklab, var(--accent) 56%, var(--panel));
    --seq-4: color-mix(in oklab, var(--accent) 78%, var(--panel));
    --seq-5: var(--accent);
  }

  header {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    gap: 12px;
  }

  header h2 {
    margin: 0 0 4px;
  }

  header p {
    margin: 0;
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
  }

  .label {
    font-size: 13px;
    color: var(--muted);
  }

  .hero .value {
    font-size: 48px;
    font-weight: 600;
    line-height: 1.1;
  }

  .tiles {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(140px, 1fr));
    gap: 12px;
  }

  .tile {
    padding: 10px 12px;
    border: 1px solid var(--border);
    border-radius: 10px;
  }

  .tile .value {
    font-size: 20px;
    font-weight: 600;
  }

  .detail {
    font-size: 12px;
    color: var(--muted);
  }

  figure {
    margin: 0;
    display: grid;
    gap: 10px;
  }

  figcaption {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    font-weight: 600;
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

  .chart {
    width: 100%;
    min-width: 0;
  }

  svg {
    display: block;
    overflow: visible;
  }

  .grid {
    stroke: var(--border);
    stroke-width: 1;
    shape-rendering: crispEdges;
  }

  .axis {
    fill: var(--muted);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
  }

  .bar {
    fill: var(--accent);
  }

  .hover-band {
    fill: var(--accent-soft);
  }

  .hit {
    fill: transparent;
  }

  .table-view {
    max-height: 240px;
    overflow-y: auto;
    border: 1px solid var(--border);
    border-radius: 10px;
  }

  table {
    width: 100%;
    border-collapse: collapse;
  }

  th,
  td {
    text-align: left;
    padding: 6px 12px;
    border-bottom: 1px solid var(--border);
  }

  th {
    position: sticky;
    top: 0;
    background: var(--panel);
    font-weight: 600;
  }

  th.num,
  td.num {
    text-align: right;
  }

  .heatmap {
    display: grid;
    grid-template-columns: auto repeat(24, minmax(0, 1fr));
    gap: 2px;
    align-items: center;
  }

  .weekday {
    padding-right: 8px;
  }

  .cell {
    height: 20px;
    border-radius: 3px;
  }

  .hour {
    font-size: 11px;
    white-space: nowrap;
    overflow: visible;
  }

  .level-0 {
    background: var(--seq-0);
  }
  .level-1 {
    background: var(--seq-1);
  }
  .level-2 {
    background: var(--seq-2);
  }
  .level-3 {
    background: var(--seq-3);
  }
  .level-4 {
    background: var(--seq-4);
  }
  .level-5 {
    background: var(--seq-5);
  }

  .cell:hover {
    outline: 2px solid var(--text);
    outline-offset: -1px;
  }

  .legend {
    display: flex;
    align-items: center;
    gap: 4px;
    flex-wrap: wrap;
  }

  .swatch {
    width: 14px;
    height: 14px;
    border-radius: 3px;
  }

  .zone {
    margin-left: auto;
  }

  .places {
    display: grid;
    grid-template-columns: 20px minmax(80px, 200px) 1fr;
    gap: 8px 10px;
    align-items: center;
  }

  .place-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .track {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }

  .hbar {
    height: 14px;
    min-width: 2px;
    max-width: calc(100% - 56px);
    background: var(--accent);
    border-radius: 0 4px 4px 0;
  }

  .tooltip {
    position: absolute;
    transform: translate(-50%, calc(-100% - 12px));
    pointer-events: none;
    background: var(--panel);
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow: var(--shadow);
    padding: 6px 10px;
    display: grid;
    gap: 2px;
    white-space: nowrap;
    z-index: 5;
  }

  @media (max-width: 640px) {
    .figures {
      grid-template-columns: 1fr;
    }
    .places {
      grid-template-columns: 20px minmax(60px, 120px) 1fr;
    }
  }
</style>
