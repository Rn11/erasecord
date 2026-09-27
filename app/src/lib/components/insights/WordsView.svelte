<script lang="ts">
  import { SvelteSet } from "svelte/reactivity";
  import { api } from "$lib/api";
  import { num, t } from "$lib/i18n.svelte";
  import { columnPath, compact, formatDate, monthName, percent, ticks } from "$lib/insights/chart";
  import { useQuery } from "$lib/insights/query.svelte";
  import { placeName } from "$lib/insights/store.svelte";
  import { hideTip, showTip } from "$lib/insights/tooltip.svelte";
  import { LENGTH_BUCKETS, type Scope } from "$lib/insights/types";
  import type { PackageSummary } from "$lib/types";
  import WordCloud from "./WordCloud.svelte";

  let {
    scope = $bindable(),
    pkg,
    onSearch,
  }: { scope: Scope; pkg: PackageSummary | null; onSearch: (words: string) => void } = $props();

  const q = useQuery("words", () => scope, api.insightsWords);
  /** Custom emoji whose picture could not be loaded (e.g. offline). */
  const broken = new SvelteSet<string>();
  const data = $derived(q.data);
  const topMax = $derived(Math.max(1, ...(data?.words ?? []).slice(0, 30).map((w) => w.count)));
  const emojiTotal = $derived(Math.max(1, data?.total_emoji ?? 1));

  // Lengths: columns, one per bucket.
  const lengthLabels = $derived(
    [...LENGTH_BUCKETS, null].map((max, i) => {
      const min = i === 0 ? 1 : LENGTH_BUCKETS[i - 1] + 1;
      return max === null ? `${num(min)}+` : `${num(min)}–${num(max)}`;
    }),
  );
  let lengthWidth = $state(500);
  const lengthChart = $derived.by(() => {
    const values = data?.lengths ?? [];
    const pad = { top: 8, bottom: 34, left: 44, right: 4 };
    const h = 190;
    const axis = ticks(Math.max(1, ...values), 3);
    const top = axis[axis.length - 1];
    const slot = (lengthWidth - pad.left - pad.right) / Math.max(1, values.length);
    const barW = Math.min(36, slot - 6);
    const base = h - pad.bottom;
    const y = (v: number) => base - (v / top) * (base - pad.top);
    return {
      h,
      pad,
      axis,
      y,
      base,
      slot,
      bars: values.map((v, i) => ({ i, v, x: pad.left + i * slot, d: columnPath(pad.left + i * slot + (slot - barW) / 2, barW, y(v), base) })),
    };
  });

  // Average length per month: one line.
  let lineWidth = $state(500);
  const lineChart = $derived.by(() => {
    const points = data?.monthly_length ?? [];
    const pad = { top: 10, bottom: 22, left: 44, right: 8 };
    const h = 170;
    const axis = ticks(Math.max(1, ...points.map((p) => p[1])), 3);
    const top = axis[axis.length - 1];
    const step = points.length > 1 ? (lineWidth - pad.left - pad.right) / (points.length - 1) : 0;
    const x = (i: number) => pad.left + (points.length > 1 ? i * step : (lineWidth - pad.left - pad.right) / 2);
    const y = (v: number) => h - pad.bottom - (v / top) * (h - pad.bottom - pad.top);
    const d = points.map((p, i) => `${i ? "L" : "M"}${x(i).toFixed(1)},${y(p[1]).toFixed(1)}`).join("");
    const labels = points
      .map((p, i) => ({ i, key: p[0] }))
      .filter(({ key, i }) => (points.length <= 14 ? true : key.endsWith("-01") || i === 0))
      .map(({ key, i }) => ({ x: x(i), text: points.length <= 14 ? formatDate(`${key}-01`, { month: "short" }) : key.slice(0, 4) }));
    return { h, pad, axis, y, x, d, labels, step, points };
  });
  let lineHover = $state<number | null>(null);
</script>

{#if q.error}
  <p class="callout error small">{q.error}</p>
{:else if !data}
  <p class="muted"><span class="spinner"></span> {t("insights.countingWords")}</p>
{:else if data.total_words === 0}
  <p class="muted">{t("insights.nothingHere")}</p>
{:else}
  <div class="words" class:stale={q.loading}>
    <div class="tiles">
      <div class="tile">
        <span class="label">{t("stats.words")}</span>
        <span class="value" title={num(data.total_words)}>{compact(data.total_words)}</span>
      </div>
      <div class="tile">
        <span class="label">{t("insights.distinctWords")}</span>
        <span class="value" title={num(data.distinct_words)}>{compact(data.distinct_words)}</span>
      </div>
      <div class="tile">
        <span class="label">{t("insights.emojiUsed")}</span>
        <span class="value">{num(data.total_emoji)}</span>
      </div>
      <div class="tile">
        <span class="label">{t("insights.peopleMentioned")}</span>
        <span class="value">{num(data.mentions.length)}{data.mentions.length >= 30 ? "+" : ""}</span>
      </div>
    </div>

    <figure>
      <figcaption>
        <span>{t("insights.wordCloud")}</span>
        <span class="muted small">{t("insights.clickWord")}</span>
      </figcaption>
      <div class="cloud-row">
        <WordCloud words={data.words} total={data.total_words} onPick={onSearch} />
        <ol class="ranking small">
          {#each data.words.slice(0, 30) as word, i (word.key)}
            <li>
              <button onclick={() => onSearch(word.key)}>
                <span class="rank muted num">{i + 1}</span>
                <span class="word">{word.key}</span>
                <span class="track"><span class="bar" style:width="{(word.count / topMax) * 100}%"></span></span>
                <span class="num">{num(word.count)}</span>
              </button>
            </li>
          {/each}
        </ol>
      </div>
      <p class="muted small">{t("insights.stopWordsNote")}</p>
    </figure>

    {#if data.emoji.length > 0}
      <figure>
        <figcaption><span>{t("insights.emoji")}</span></figcaption>
        <div class="emoji">
          {#each data.emoji.slice(0, 40) as e (e.emoji + (e.id ?? ""))}
            <span
              class="chip"
              role="img"
              aria-label="{e.id ? `:${e.emoji}:` : e.emoji} {num(e.count)}"
              onpointermove={(ev) =>
                showTip(ev, e.id ? `:${e.emoji}:` : e.emoji, [
                  { label: t("insights.times"), value: num(e.count) },
                  { label: t("insights.ofAllEmoji"), value: percent(e.count / emojiTotal) },
                ])}
              onpointerleave={hideTip}
            >
              {#if e.id && !broken.has(e.id)}
                <img
                  onerror={() => broken.add(e.id ?? "")}
                  src="https://cdn.discordapp.com/emojis/{e.id}.{e.animated ? 'gif' : 'webp'}?size=48"
                  alt=":{e.emoji}:"
                  loading="lazy"
                  width="22"
                  height="22"
                />
              {:else if e.id}
                <span class="name-only">:{e.emoji}:</span>
              {:else}
                <span class="glyph">{e.emoji}</span>
              {/if}
              <span class="num small">{compact(e.count)}</span>
            </span>
          {/each}
        </div>
        {#if data.emoji.some((e) => e.id)}<p class="muted small">{t("insights.customEmojiNote")}</p>{/if}
      </figure>
    {/if}

    {#if data.mentions.length > 0}
      <figure>
        <figcaption><span>{t("insights.mentions")}</span></figcaption>
        <ol class="mentions small">
          {#each data.mentions.slice(0, 12) as m (m.id)}
            <li>
              <span class="word">{m.name ?? t("insights.unknownUser", { id: m.id.slice(-4) })}</span>
              <span class="num muted">{t("insights.mentionCount", { count: m.count })}</span>
            </li>
          {/each}
        </ol>
      </figure>
    {/if}

    <div class="pair">
      <figure>
        <figcaption><span>{t("insights.lengths")}</span></figcaption>
        <div bind:clientWidth={lengthWidth}>
          <svg width={lengthWidth} height={lengthChart.h} role="img" aria-label={t("insights.lengths")}>
            {#each lengthChart.axis as tick (tick)}
              <line class="grid" x1={lengthChart.pad.left} x2={lengthWidth - lengthChart.pad.right} y1={lengthChart.y(tick)} y2={lengthChart.y(tick)} />
              <text class="axis" x={lengthChart.pad.left - 8} y={lengthChart.y(tick)} dy="0.32em" text-anchor="end">{compact(tick)}</text>
            {/each}
            {#each lengthChart.bars as bar (bar.i)}
              <path d={bar.d} class="bar-fill" />
              <text class="axis" x={bar.x + lengthChart.slot / 2} y={lengthChart.h - 18} text-anchor="middle">{lengthLabels[bar.i]}</text>
              <rect
                class="hit"
                role="presentation"
                x={bar.x}
                y={lengthChart.pad.top}
                width={lengthChart.slot}
                height={lengthChart.base - lengthChart.pad.top}
                onpointermove={(e) =>
                  showTip(e, t("insights.charactersRange", { range: lengthLabels[bar.i] }), [
                    { label: t("stats.messages"), value: num(bar.v) },
                  ])}
                onpointerleave={hideTip}
              />
            {/each}
            <text class="axis" x={lengthChart.pad.left + (lengthWidth - lengthChart.pad.left) / 2} y={lengthChart.h - 2} text-anchor="middle">
              {t("insights.characters")}
            </text>
          </svg>
        </div>
      </figure>

      {#if lineChart.points.length > 1}
        <figure>
          <figcaption><span>{t("insights.averageLength")}</span></figcaption>
          <div bind:clientWidth={lineWidth}>
            <svg width={lineWidth} height={lineChart.h} role="img" aria-label={t("insights.averageLength")}>
              {#each lineChart.axis as tick (tick)}
                <line class="grid" x1={lineChart.pad.left} x2={lineWidth - lineChart.pad.right} y1={lineChart.y(tick)} y2={lineChart.y(tick)} />
                <text class="axis" x={lineChart.pad.left - 8} y={lineChart.y(tick)} dy="0.32em" text-anchor="end">{compact(tick)}</text>
              {/each}
              {#if lineHover !== null}
                <line class="crosshair" x1={lineChart.x(lineHover)} x2={lineChart.x(lineHover)} y1={lineChart.pad.top} y2={lineChart.h - lineChart.pad.bottom} />
              {/if}
              <path d={lineChart.d} class="line" />
              {#if lineHover !== null}
                <circle cx={lineChart.x(lineHover)} cy={lineChart.y(lineChart.points[lineHover][1])} r="4" class="dot" />
              {/if}
              {#each lineChart.labels as label (label.x)}
                <text class="axis" x={label.x} y={lineChart.h - 6} text-anchor="middle">{label.text}</text>
              {/each}
              <rect
                class="hit"
                role="presentation"
                x={lineChart.pad.left}
                y={lineChart.pad.top}
                width={Math.max(0, lineWidth - lineChart.pad.left - lineChart.pad.right)}
                height={lineChart.h - lineChart.pad.bottom - lineChart.pad.top}
                onpointermove={(e) => {
                  const box = (e.currentTarget as SVGRectElement).getBoundingClientRect();
                  const i = Math.round(((e.clientX - box.left) / Math.max(1, box.width)) * (lineChart.points.length - 1));
                  lineHover = Math.max(0, Math.min(lineChart.points.length - 1, i));
                  const [key, value] = lineChart.points[lineHover];
                  showTip(e, monthName(key), [{ label: t("insights.averageChars"), value: num(Math.round(value)) }]);
                }}
                onpointerleave={() => ((lineHover = null), hideTip())}
              />
            </svg>
          </div>
        </figure>
      {/if}
    </div>

    {#if data.longest}
      <figure class="message">
        <figcaption class="plain">
          <span class="label">{t("insights.longest", { count: data.longest.text.length })}</span>
          <span class="muted small">
            {formatDate(data.longest.sent_at, { dateStyle: "long", timeStyle: "short" })} · {placeName(data.longest.place, pkg)}
          </span>
        </figcaption>
        <blockquote>{data.longest.text}</blockquote>
      </figure>
    {/if}
  </div>
{/if}

<style>
  .words {
    display: grid;
    gap: 26px;
    transition: opacity 0.15s;
  }

  .stale {
    opacity: 0.6;
  }

  .tiles {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
    gap: 10px;
  }

  .tile {
    display: grid;
    gap: 2px;
    padding: 10px 12px;
    border: 1px solid var(--border);
    border-radius: 10px;
  }

  .label {
    font-size: 13px;
    color: var(--muted);
  }

  .tile .value {
    font-size: 19px;
    font-weight: 600;
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

  figcaption.plain {
    display: grid;
    gap: 2px;
    font-weight: 400;
  }

  figure p {
    margin: 0;
  }

  .cloud-row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 260px;
    gap: 18px;
    align-items: start;
  }

  .ranking,
  .mentions {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 1px;
  }

  .ranking {
    max-height: 340px;
    overflow-y: auto;
  }

  .ranking button {
    width: 100%;
    display: grid;
    grid-template-columns: 22px minmax(0, 100px) minmax(0, 1fr) auto;
    gap: 8px;
    align-items: center;
    border: none;
    background: none;
    font: inherit;
    color: inherit;
    padding: 2px 4px;
    border-radius: 4px;
    cursor: pointer;
    text-align: left;
  }

  .ranking button:hover {
    background: var(--panel-2);
  }

  .word {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .rank {
    text-align: right;
  }

  .track {
    display: block;
  }

  .bar {
    display: block;
    height: 8px;
    background: var(--accent);
    border-radius: 0 4px 4px 0;
    min-width: 2px;
  }

  .emoji {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 4px 10px 4px 6px;
    border: 1px solid var(--border);
    border-radius: 999px;
  }

  .name-only {
    font-size: 12px;
    color: var(--muted);
  }

  .glyph {
    font-size: 20px;
    line-height: 22px;
  }

  .mentions {
    grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
    gap: 6px 18px;
  }

  .mentions li {
    display: flex;
    justify-content: space-between;
    gap: 8px;
    padding: 4px 0;
    border-bottom: 1px solid var(--border);
  }

  .pair {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(320px, 1fr));
    gap: 24px;
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

  .bar-fill {
    fill: var(--accent);
  }

  .line {
    fill: none;
    stroke: var(--accent);
    stroke-width: 2;
    stroke-linejoin: round;
    stroke-linecap: round;
  }

  .crosshair {
    stroke: var(--muted);
    stroke-width: 1;
  }

  .dot {
    fill: var(--accent);
    stroke: var(--panel);
    stroke-width: 2;
  }

  .hit {
    fill: transparent;
  }

  .message {
    padding: 12px 14px;
    border: 1px solid var(--border);
    border-radius: 10px;
  }

  blockquote {
    margin: 0;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    max-height: 14em;
    overflow-y: auto;
  }

  @media (max-width: 820px) {
    .cloud-row {
      grid-template-columns: 1fr;
    }
  }
</style>
