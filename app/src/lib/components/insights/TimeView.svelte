<script lang="ts">
  import { api } from "$lib/api";
  import { num, t } from "$lib/i18n.svelte";
  import {
    addDays,
    columnPath,
    compact,
    formatDate,
    hourLabel,
    levels,
    localDate,
    monthEnd,
    monthName,
    SERIES_COLORS,
    ticks,
    weekdays,
  } from "$lib/insights/chart";
  import { useQuery } from "$lib/insights/query.svelte";
  import { placeName } from "$lib/insights/store.svelte";
  import { hideTip, showTip } from "$lib/insights/tooltip.svelte";
  import type { Scope } from "$lib/insights/types";
  import type { PackageSummary } from "$lib/types";
  import CalendarHeatmap from "./CalendarHeatmap.svelte";
  import StackedColumns from "./StackedColumns.svelte";

  let { scope = $bindable(), pkg }: { scope: Scope; pkg: PackageSummary | null } = $props();

  const q = useQuery("time", () => scope, api.insightsTime);
  const data = $derived(q.data);

  let granularity = $state<"auto" | "month" | "week">("auto");
  const byWeek = $derived(granularity === "week" || (granularity === "auto" && (data?.months.length ?? 0) <= 6));
  const columns = $derived((byWeek ? data?.weeks : data?.months) ?? []);
  const series = $derived(
    (data?.series ?? []).map((s) => ({ label: placeName(s.place, pkg), color: SERIES_COLORS[s.slot] })),
  );

  function axisLabel(key: string, index: number): string | null {
    const date = localDate(key);
    if (byWeek) {
      if (columns.length <= 16) return formatDate(date, { day: "numeric", month: "short" });
      const previous = index > 0 ? localDate(columns[index - 1].key) : null;
      if (previous && previous.getMonth() === date.getMonth()) return null;
      if (columns.length > 30 && date.getMonth() % 3 !== 0) return null;
      return date.getMonth() === 0 || index === 0 ? formatDate(date, { month: "short", year: "numeric" }) : formatDate(date, { month: "short" });
    }
    if (columns.length <= 14) return formatDate(date, { month: "short" });
    if (date.getMonth() !== 0 && index !== 0) return null;
    if (index === 0 && date.getMonth() > 8 && columns.length > 12) return null;
    return String(date.getFullYear());
  }

  function title(key: string): string {
    if (!byWeek) return monthName(key);
    return t("insights.weekOf", { date: formatDate(key) });
  }

  function pick(key: string) {
    if (byWeek) Object.assign(scope, { from: key, to: addDays(key, 6) });
    else Object.assign(scope, { from: `${key}-01`, to: monthEnd(key) });
  }

  // Weekday × hour.
  const names = $derived(weekdays("short"));
  const longNames = $derived(weekdays("long"));
  const level = $derived(levels((data?.week_hours ?? []).flat()));
  const hours = $derived(Array.from({ length: 24 }, (_, h) => (data?.week_hours ?? []).reduce((s, d) => s + d[h], 0)));
  const hoursTotal = $derived(hours.reduce((a, b) => a + b, 0));

  let hourWidth = $state(600);
  const hourChart = $derived.by(() => {
    const pad = { top: 8, bottom: 22, left: 40, right: 4 };
    const h = 150;
    const axis = ticks(Math.max(...hours, 1), 3);
    const top = axis[axis.length - 1];
    const plotW = Math.max(1, hourWidth - pad.left - pad.right);
    const slot = plotW / 24;
    const barW = Math.min(24, slot - 2);
    const base = h - pad.bottom;
    const y = (v: number) => base - (v / top) * (base - pad.top);
    return {
      h,
      pad,
      axis,
      y,
      slot,
      base,
      bars: hours.map((v, i) => ({ i, v, x: pad.left + i * slot, d: columnPath(pad.left + i * slot + (slot - barW) / 2, barW, y(v), base) })),
    };
  });
  const peak = $derived(hours.indexOf(Math.max(...hours)));
</script>

{#if q.error}
  <p class="callout error small">{q.error}</p>
{:else if !data}
  <p class="muted"><span class="spinner"></span> {t("stats.loading")}</p>
{:else if data.days.length === 0}
  <p class="muted">{t("insights.nothingHere")}</p>
{:else}
  <div class="time" class:stale={q.loading}>
    <figure>
      <figcaption>
        <span>{t("insights.overTime")}</span>
        <span class="segmented small" role="radiogroup" aria-label={t("insights.granularity")}>
          <button role="radio" aria-checked={!byWeek} class:on={!byWeek} onclick={() => (granularity = "month")}>
            {t("insights.months")}
          </button>
          <button role="radio" aria-checked={byWeek} class:on={byWeek} onclick={() => (granularity = "week")}>
            {t("insights.weeks")}
          </button>
        </span>
      </figcaption>
      <StackedColumns
        {columns}
        {series}
        {axisLabel}
        {title}
        onPick={pick}
        pickHint={byWeek ? t("insights.clickWeek") : t("insights.clickMonth")}
      />
    </figure>

    <figure>
      <figcaption><span>{t("insights.everyDay")}</span></figcaption>
      <CalendarHeatmap days={data.days} onPick={(day) => Object.assign(scope, { from: day, to: day })} />
    </figure>

    <figure>
      <figcaption>
        <span>{t("stats.week")}</span>
        <span class="muted small">{t("stats.timeZone")}</span>
      </figcaption>
      <div class="heatmap" role="table" aria-label={t("stats.week")}>
        {#each data.week_hours as row, d (d)}
          <div class="weekday small muted" role="rowheader">{names[d]}</div>
          {#each row as value, h (h)}
            <div
              class="cell level-{level(value)}"
              role="cell"
              tabindex="-1"
              aria-label="{longNames[d]} {hourLabel(h)}: {t('count.message', { count: value })}"
              onpointermove={(e) =>
                showTip(e, t("stats.hourRange", { day: longNames[d], from: hourLabel(h), to: hourLabel((h + 1) % 24) }), [
                  { label: t("stats.messages"), value: num(value) },
                ])}
              onpointerleave={hideTip}
            ></div>
          {/each}
        {/each}
        <div></div>
        {#each Array.from({ length: 24 }, (_, h) => h) as h (h)}
          <div class="hour small muted">{h % 6 === 0 ? hourLabel(h) : ""}</div>
        {/each}
      </div>
      <div class="legend small muted">
        <span>{t("stats.fewer")}</span>
        {#each [0, 1, 2, 3, 4, 5] as l (l)}<span class="swatch level-{l}"></span>{/each}
        <span>{t("stats.more")}</span>
      </div>
    </figure>

    <figure>
      <figcaption>
        <span>{t("insights.byHour")}</span>
        {#if hoursTotal > 0}
          <span class="muted small">{t("insights.peakHour", { hour: hourLabel(peak) })}</span>
        {/if}
      </figcaption>
      <div class="hours" bind:clientWidth={hourWidth}>
        <svg width={hourWidth} height={hourChart.h} role="img" aria-label={t("insights.byHour")}>
          {#each hourChart.axis as tick (tick)}
            <line class="grid" x1={hourChart.pad.left} x2={hourWidth - hourChart.pad.right} y1={hourChart.y(tick)} y2={hourChart.y(tick)} />
            <text class="axis" x={hourChart.pad.left - 8} y={hourChart.y(tick)} dy="0.32em" text-anchor="end">{compact(tick)}</text>
          {/each}
          {#each hourChart.bars as bar (bar.i)}
            <path d={bar.d} class="bar" />
            {#if bar.i % 3 === 0}
              <text class="axis" x={bar.x + hourChart.slot / 2} y={hourChart.h - 6} text-anchor="middle">{hourLabel(bar.i)}</text>
            {/if}
            <rect
              class="hit"
              role="presentation"
              x={bar.x}
              y={hourChart.pad.top}
              width={hourChart.slot}
              height={hourChart.base - hourChart.pad.top}
              onpointermove={(e) =>
                showTip(e, `${hourLabel(bar.i)} – ${hourLabel((bar.i + 1) % 24)}`, [
                  { label: t("stats.messages"), value: num(bar.v) },
                  { label: t("insights.share"), value: `${Math.round((bar.v / Math.max(1, hoursTotal)) * 1000) / 10} %` },
                ])}
              onpointerleave={hideTip}
            />
          {/each}
        </svg>
      </div>
    </figure>
  </div>
{/if}

<style>
  .time {
    display: grid;
    gap: 26px;
    transition: opacity 0.15s;
    /* One hue, light to dark; step 0 means none. */
    --seq-0: color-mix(in oklab, var(--border) 45%, var(--panel));
    --seq-1: color-mix(in oklab, var(--accent) 18%, var(--panel));
    --seq-2: color-mix(in oklab, var(--accent) 36%, var(--panel));
    --seq-3: color-mix(in oklab, var(--accent) 56%, var(--panel));
    --seq-4: color-mix(in oklab, var(--accent) 78%, var(--panel));
    --seq-5: var(--accent);
  }

  .stale {
    opacity: 0.6;
  }

  figure {
    margin: 0;
    display: grid;
    gap: 10px;
    min-width: 0;
  }

  figcaption {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 12px;
    font-weight: 600;
  }

  .segmented {
    display: inline-flex;
    border: 1px solid var(--border);
    border-radius: 8px;
    overflow: hidden;
    font-weight: 400;
  }

  .segmented button {
    border: none;
    background: none;
    font: inherit;
    color: var(--muted);
    padding: 3px 10px;
    cursor: pointer;
  }

  .segmented button.on {
    background: var(--accent-soft);
    color: var(--text);
    font-weight: 600;
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

  .cell:hover {
    outline: 2px solid var(--text);
    outline-offset: -1px;
  }

  .hour {
    font-size: 11px;
    white-space: nowrap;
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

  .legend {
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .swatch {
    width: 12px;
    height: 12px;
    border-radius: 2px;
  }

  .hours {
    width: 100%;
    min-width: 0;
  }

  svg {
    display: block;
    overflow: visible;
  }

  .grid {
    stroke: var(--grid);
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

  .hit {
    fill: transparent;
  }
</style>
