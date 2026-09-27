<script lang="ts">
  import { formatDuration } from "$lib/format";
  import { i18n, num, t, type Key } from "$lib/i18n.svelte";
  import type { ScanStats } from "$lib/types";

  // What the messages found so far look like: numbers, when they were
  // written, and what is in them.
  let {
    stats,
    partial,
    deleteMs,
  }: {
    stats: ScanStats;
    /** Not everything is read yet. */
    partial: boolean;
    /** Roughly how long deleting them takes. */
    deleteMs: number | null;
  } = $props();

  const dateFmt = $derived(new Intl.DateTimeFormat(i18n.locale, { year: "numeric", month: "short", day: "numeric" }));
  const monthFmt = $derived(new Intl.DateTimeFormat(i18n.locale, { year: "numeric", month: "short" }));
  const dayFmt = $derived(new Intl.DateTimeFormat(i18n.locale, { weekday: "short" }));
  const date = (iso: string) => dateFmt.format(new Date(iso));
  const month = (key: string) => monthFmt.format(new Date(`${key}-15T12:00:00`));

  // Months without messages count too, so the chart shows gaps.
  const months = $derived.by(() => {
    if (stats.months.length === 0) return [] as [string, number][];
    const counts = new Map(stats.months);
    const [fy, fm] = stats.months[0][0].split("-").map(Number);
    const [ly, lm] = stats.months[stats.months.length - 1][0].split("-").map(Number);
    const all: [string, number][] = [];
    for (let y = fy, m = fm; y < ly || (y === ly && m <= lm); m === 12 ? (y++, (m = 1)) : m++) {
      const key = `${y}-${String(m).padStart(2, "0")}`;
      all.push([key, counts.get(key) ?? 0]);
      if (all.length > 240) break;
    }
    return all;
  });
  const monthMax = $derived(Math.max(1, ...months.map(([, n]) => n)));
  const weekMax = $derived(Math.max(1, ...stats.week.flat()));
  // Monday 2024-01-01 onward, for weekday names.
  const weekdays = $derived(Array.from({ length: 7 }, (_, i) => dayFmt.format(new Date(2024, 0, 1 + i))));
  const wordMax = $derived(Math.max(1, ...stats.top_words.map(([, n]) => n)));
  const busiestHour = $derived.by(() => {
    const hours = Array.from({ length: 24 }, (_, h) => stats.week.reduce((sum, row) => sum + (row[h] ?? 0), 0));
    const max = Math.max(...hours);
    return max > 0 ? hours.indexOf(max) : null;
  });
  const avgWords = $derived(stats.messages > 0 ? stats.words / stats.messages : 0);
  const kinds = $derived(
    [
      ["found.files", stats.with_files],
      ["found.images", stats.images],
      ["found.videos", stats.videos],
      ["found.audio", stats.audio],
      ["found.links", stats.links],
    ] as [string, number][],
  );
</script>

<section class="stats" aria-label={t("found.label")}>
  <div class="tiles">
    <div class="tile">
      <span class="big num">{num(stats.messages)}</span>
      <span class="muted small">{partial ? t("found.messagesSoFar") : t("found.messages")}</span>
    </div>
    <div class="tile">
      <span class="big num">{num(stats.words)}</span>
      <span class="muted small">{t("found.words", { avg: num(Math.round(avgWords * 10) / 10) })}</span>
    </div>
    {#if stats.first && stats.last}
      <div class="tile">
        <span class="mid">{date(stats.first)} – {date(stats.last)}</span>
        <span class="muted small">{t("found.span")}</span>
      </div>
    {/if}
    {#if deleteMs !== null && stats.messages > 0}
      <div class="tile">
        <span class="mid">{t("found.about", { time: formatDuration(deleteMs) })}</span>
        <span class="muted small">{t("found.deleteTime")}</span>
      </div>
    {/if}
  </div>

  {#if months.length > 1}
    <figure>
      <figcaption class="small muted">{t("found.perMonth")}</figcaption>
      <div class="months" role="img" aria-label={t("found.perMonth")}>
        {#each months as [key, n] (key)}
          <span class="bar" style:height="{Math.max(n > 0 ? 4 : 0, (n / monthMax) * 100)}%" title="{month(key)}: {num(n)}"></span>
        {/each}
      </div>
      <div class="axis small muted">
        <span>{month(months[0][0])}</span>
        <span>{month(months[months.length - 1][0])}</span>
      </div>
    </figure>
  {/if}

  <div class="two">
    {#if stats.messages > 0}
      <figure>
        <figcaption class="small muted">
          {t("found.week")}
          {#if busiestHour !== null}· {t("found.busiestHour", { hour: busiestHour })}{/if}
        </figcaption>
        <div class="week" role="img" aria-label={t("found.week")}>
          {#each stats.week as row, d (d)}
            <span class="day small muted">{weekdays[d]}</span>
            {#each row as n, h (h)}
              <span
                class="cell"
                style:opacity={n === 0 ? 0.08 : 0.2 + 0.8 * (n / weekMax)}
                title="{weekdays[d]} {String(h).padStart(2, '0')}:00 · {num(n)}"
              ></span>
            {/each}
          {/each}
        </div>
      </figure>
    {/if}

    <div class="facts small">
      {#each kinds as [key, n] (key)}
        {#if n > 0}<span class="chip">{t(key as Key, { count: n })}</span>{/if}
      {/each}
      {#if stats.busiest_day}
        <p>{t("found.busiestDay", { date: date(`${stats.busiest_day[0]}T12:00:00`), count: stats.busiest_day[1] })}</p>
      {/if}
      {#if stats.longest > 0}
        <p>{t("found.longest", { count: stats.longest })}</p>
      {/if}
      {#if stats.pinned_kept > 0}
        <p>{t("found.pinnedKept", { count: stats.pinned_kept })}</p>
      {/if}
      {#if stats.top_emoji.length > 0}
        <p class="emoji" title={t("found.emoji")}>
          {#each stats.top_emoji as [e, n] (e)}<span title={num(n)}>{e}</span>{/each}
        </p>
      {/if}
    </div>
  </div>

  {#if stats.top_words.length > 0}
    <figure>
      <figcaption class="small muted">{t("found.topWords")}</figcaption>
      <p class="words">
        {#each stats.top_words as [word, n] (word)}
          <span style:font-size="{0.8 + 0.7 * (n / wordMax)}em" style:opacity={0.55 + 0.45 * (n / wordMax)} title={num(n)}>{word}</span>
        {/each}
      </p>
    </figure>
  {/if}
</section>

<style>
  .stats {
    display: grid;
    gap: 14px;
  }

  .tiles {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(150px, 1fr));
    gap: 8px;
  }

  .tile {
    display: grid;
    gap: 2px;
    padding: 10px 12px;
    border: 1px solid var(--border);
    border-radius: 10px;
    background: var(--panel-2);
  }

  .big {
    font-size: 22px;
    font-weight: 700;
  }

  .mid {
    font-weight: 600;
  }

  figure {
    margin: 0;
    display: grid;
    gap: 6px;
  }

  .months {
    height: 70px;
    display: flex;
    align-items: flex-end;
    gap: 2px;
  }

  .bar {
    flex: 1;
    min-width: 2px;
    background: var(--accent);
    border-radius: 2px 2px 0 0;
    transition: height 0.3s;
  }

  .axis {
    display: flex;
    justify-content: space-between;
  }

  .two {
    display: grid;
    grid-template-columns: minmax(0, 1.4fr) minmax(0, 1fr);
    gap: 16px;
    align-items: start;
  }

  @media (max-width: 700px) {
    .two {
      grid-template-columns: 1fr;
    }
  }

  .week {
    display: grid;
    grid-template-columns: auto repeat(24, 1fr);
    gap: 2px;
    align-items: center;
  }

  .day {
    padding-right: 4px;
    font-size: 11px;
  }

  .cell {
    aspect-ratio: 1;
    border-radius: 2px;
    background: var(--accent);
  }

  .facts {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    align-content: start;
  }

  .facts p {
    margin: 0;
    flex-basis: 100%;
  }

  .chip {
    padding: 2px 8px;
    border-radius: 999px;
    border: 1px solid var(--border);
  }

  .emoji {
    font-size: 18px;
    display: flex;
    gap: 6px;
  }

  .words {
    margin: 0;
    display: flex;
    flex-wrap: wrap;
    gap: 2px 12px;
    align-items: baseline;
    line-height: 1.3;
  }

  @media (prefers-reduced-motion: reduce) {
    .bar {
      transition: none;
    }
  }
</style>
