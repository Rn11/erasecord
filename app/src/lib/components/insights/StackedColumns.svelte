<script lang="ts">
  import { num, t } from "$lib/i18n.svelte";
  import { columnPath, compact, OTHER_COLOR, ticks } from "$lib/insights/chart";
  import { hideTip, showTip } from "$lib/insights/tooltip.svelte";
  import type { Bucket } from "$lib/insights/types";

  let {
    columns,
    series,
    axisLabel,
    title,
    onPick,
    pickHint,
    height = 220,
  }: {
    /** Per column: the total and each series; the rest is "other". */
    columns: Bucket[];
    series: { label: string; color: string }[];
    /** Text under the column, or null for none. */
    axisLabel: (key: string, index: number) => string | null;
    title: (key: string) => string;
    onPick?: (key: string) => void;
    pickHint?: string;
    height?: number;
  } = $props();

  const PAD = { top: 10, right: 6, bottom: 24, left: 44 };
  let width = $state(640);
  let table = $state(false);
  let hovered = $state<number | null>(null);

  const withOther = $derived(columns.some((c) => c.total > c.series.reduce((a, b) => a + b, 0)));
  const layers = $derived([
    ...series,
    ...(withOther ? [{ label: t("insights.other"), color: OTHER_COLOR }] : []),
  ]);

  const chart = $derived.by(() => {
    const max = Math.max(1, ...columns.map((c) => c.total));
    const axis = ticks(max);
    const top = axis[axis.length - 1];
    const plotW = Math.max(1, width - PAD.left - PAD.right);
    const plotH = height - PAD.top - PAD.bottom;
    const base = PAD.top + plotH;
    const slot = plotW / Math.max(1, columns.length);
    const barW = Math.max(1, Math.min(24, slot - (slot > 4 ? 2 : 0.5)));
    const y = (v: number) => base - (v / top) * plotH;
    const bars = columns.map((c, i) => {
      const values = [...c.series, c.total - c.series.reduce((a, b) => a + b, 0)];
      const x = PAD.left + i * slot + (slot - barW) / 2;
      let sum = 0;
      const last = values.reduce((l, v, j) => (v > 0 ? j : l), -1);
      const segments = values
        .map((v, j) => {
          if (v <= 0) return null;
          const bottom = y(sum);
          sum += v;
          const topY = y(sum);
          // A 2px gap of surface between stacked segments.
          const gap = j !== last && bottom - topY > 3 ? 2 : 0;
          return { d: columnPath(x, barW, topY + gap, bottom, j === last), color: layers[j]?.color ?? OTHER_COLOR };
        })
        .filter((s) => s !== null);
      return { key: c.key, i, x: PAD.left + i * slot, segments, values };
    });
    const labels = columns
      .map((c, i) => ({ text: axisLabel(c.key, i), x: PAD.left + i * slot + slot / 2 }))
      .filter((l) => l.text !== null);
    return { axis, y, bars, labels, slot, plotH, base };
  });

  function tipFor(event: PointerEvent, bar: (typeof chart.bars)[number]) {
    hovered = bar.i;
    const column = columns[bar.i];
    const lines = [
      { label: t("preview.total"), value: num(column.total) },
      ...bar.values
        .map((v, j) => ({ label: layers[j]?.label ?? "", value: num(v), color: layers[j]?.color }))
        .filter((l) => l.value !== "0" && l.label),
    ];
    showTip(event, title(column.key), lines);
  }
</script>

<div class="stacked">
  <div class="tools small">
    <div class="legend" aria-label={t("insights.legend")}>
      {#each layers as layer (layer.label)}
        <span class="key"><span class="swatch" style:background={layer.color}></span>{layer.label}</span>
      {/each}
    </div>
    <button class="link small" onclick={() => (table = !table)}>
      {table ? t("stats.hideTable") : t("stats.showTable")}
    </button>
  </div>

  {#if table}
    <div class="table-view">
      <table class="small">
        <thead>
          <tr>
            <th></th>
            <th class="num">{t("preview.total")}</th>
            {#each layers as layer (layer.label)}<th class="num">{layer.label}</th>{/each}
          </tr>
        </thead>
        <tbody>
          {#each [...chart.bars].reverse() as bar (bar.key)}
            <tr>
              <td>{title(bar.key)}</td>
              <td class="num">{num(columns[bar.i].total)}</td>
              {#each layers as layer, j (layer.label)}<td class="num">{num(bar.values[j] ?? 0)}</td>{/each}
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {:else}
    <div class="chart" bind:clientWidth={width}>
      <svg {width} {height} role="img" aria-label={pickHint}>
        {#each chart.axis as tick (tick)}
          <line class="grid" x1={PAD.left} x2={width - PAD.right} y1={chart.y(tick)} y2={chart.y(tick)} />
          <text class="axis" x={PAD.left - 8} y={chart.y(tick)} dy="0.32em" text-anchor="end">{compact(tick)}</text>
        {/each}
        {#each chart.bars as bar (bar.key)}
          {#if hovered === bar.i}
            <rect class="band" x={bar.x} y={PAD.top} width={chart.slot} height={chart.plotH} />
          {/if}
          {#each bar.segments as segment, j (j)}
            <path d={segment.d} fill={segment.color} />
          {/each}
        {/each}
        {#each chart.labels as label, i (i)}
          <text class="axis" x={label.x} y={height - 6} text-anchor="middle">{label.text}</text>
        {/each}
        {#each chart.bars as bar (bar.key)}
          <rect
            class="hit"
            class:pickable={!!onPick}
            role="presentation"
            x={bar.x}
            y={PAD.top}
            width={chart.slot}
            height={chart.plotH}
            onpointermove={(e) => tipFor(e, bar)}
            onpointerleave={() => ((hovered = null), hideTip())}
            onclick={() => {
              hideTip();
              onPick?.(bar.key);
            }}
          />
        {/each}
      </svg>
    </div>
    {#if pickHint}<p class="muted small hint">{pickHint}</p>{/if}
  {/if}
</div>

<style>
  .stacked {
    display: grid;
    gap: 8px;
  }

  .tools {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    gap: 12px;
  }

  .legend {
    display: flex;
    flex-wrap: wrap;
    gap: 4px 14px;
  }

  .key {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    max-width: 220px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .swatch {
    width: 10px;
    height: 10px;
    border-radius: 3px;
    flex: none;
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
    stroke: var(--grid);
    stroke-width: 1;
    shape-rendering: crispEdges;
  }

  .axis {
    fill: var(--muted);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
  }

  .band {
    fill: var(--accent-soft);
  }

  .hit {
    fill: transparent;
  }

  .hit.pickable {
    cursor: pointer;
  }

  .hint {
    margin: 0;
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

  .table-view {
    max-height: 300px;
    overflow: auto;
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
    padding: 5px 10px;
    border-bottom: 1px solid var(--border);
    white-space: nowrap;
  }

  th {
    position: sticky;
    top: 0;
    background: var(--panel);
    font-weight: 600;
  }

  .num {
    text-align: right;
  }
</style>
