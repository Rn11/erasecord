<script lang="ts">
  import { num, t } from "$lib/i18n.svelte";
  import { formatDate, isoDay, levels, localDate, weekdays } from "$lib/insights/chart";
  import { hideTip, showTip } from "$lib/insights/tooltip.svelte";
  import type { DayCount } from "$lib/insights/types";

  let { days, onPick }: { days: DayCount[]; onPick?: (day: string) => void } = $props();

  let width = $state(700);
  const counts = $derived(new Map(days.map((d) => [d.date, d.messages])));
  const level = $derived(levels(days.map((d) => d.messages)));
  const names = $derived(weekdays("short"));

  const years = $derived.by(() => {
    if (days.length === 0) return [];
    const first = localDate(days[0].date).getFullYear();
    const last = localDate(days[days.length - 1].date).getFullYear();
    return Array.from({ length: last - first + 1 }, (_, i) => last - i).map((year) => {
      // Columns are weeks from Monday; the first may start in December.
      const start = new Date(year, 0, 1);
      start.setDate(start.getDate() - ((start.getDay() + 6) % 7));
      const weeks: { day: string; inYear: boolean }[][] = [];
      const months: { column: number; label: string }[] = [];
      const cursor = new Date(start);
      while (cursor.getFullYear() <= year) {
        const week = [];
        for (let d = 0; d < 7; d++) {
          const inYear = cursor.getFullYear() === year;
          if (inYear && cursor.getDate() === 1) {
            months.push({ column: weeks.length, label: formatDate(cursor, { month: "short" }) });
          }
          week.push({ day: isoDay(cursor), inYear });
          cursor.setDate(cursor.getDate() + 1);
        }
        weeks.push(week);
        if (cursor.getFullYear() > year) break;
      }
      const total = [...counts].reduce((sum, [day, n]) => (day.startsWith(`${year}-`) ? sum + n : sum), 0);
      return { year, weeks, months, total };
    });
  });

  const cell = $derived(Math.max(6, Math.min(14, Math.floor((width - 44) / 54) - 2)));
</script>

<div class="calendar" bind:clientWidth={width}>
  {#each years as year (year.year)}
    <div class="year">
      <div class="year-head small">
        <strong class="num">{year.year}</strong>
        <span class="muted">{t("count.message", { count: year.total })}</span>
      </div>
      <div class="grid" style:--cell="{cell}px" style:--weeks={year.weeks.length}>
        <div class="months">
          {#each year.months as month (month.column)}
            <span class="month small muted" style:grid-column="{month.column + 2}">{month.label}</span>
          {/each}
        </div>
        {#each [0, 2, 4] as d (d)}
          <span class="weekday small muted" style:grid-row="{d + 2}">{names[d]}</span>
        {/each}
        {#each year.weeks as week, w (w)}
          {#each week as day, d (day.day)}
            {#if day.inYear}
              {@const n = counts.get(day.day) ?? 0}
              <button
                class="cell level-{level(n)}"
                style:grid-column={w + 2}
                style:grid-row={d + 2}
                aria-label="{formatDate(day.day, { dateStyle: 'full' })}: {t('count.message', { count: n })}"
                onpointermove={(e) =>
                  showTip(e, formatDate(day.day, { dateStyle: "full" }), [
                    { label: t("stats.messages"), value: num(n) },
                  ])}
                onpointerleave={hideTip}
                onclick={() => {
                  hideTip();
                  if (n > 0) onPick?.(day.day);
                }}
              ></button>
            {/if}
          {/each}
        {/each}
      </div>
    </div>
  {/each}
  <div class="legend small muted">
    <span>{t("stats.fewer")}</span>
    {#each [0, 1, 2, 3, 4, 5] as l (l)}<span class="swatch level-{l}"></span>{/each}
    <span>{t("stats.more")}</span>
    {#if onPick}<span class="hint">{t("insights.clickDay")}</span>{/if}
  </div>
</div>

<style>
  .calendar {
    display: grid;
    gap: 14px;
    min-width: 0;
    /* One hue, light to dark; step 0 means no messages. */
    --seq-0: color-mix(in oklab, var(--border) 45%, var(--panel));
    --seq-1: color-mix(in oklab, var(--accent) 18%, var(--panel));
    --seq-2: color-mix(in oklab, var(--accent) 36%, var(--panel));
    --seq-3: color-mix(in oklab, var(--accent) 56%, var(--panel));
    --seq-4: color-mix(in oklab, var(--accent) 78%, var(--panel));
    --seq-5: var(--accent);
  }

  .year {
    display: grid;
    gap: 4px;
  }

  .year-head {
    display: flex;
    gap: 10px;
    align-items: baseline;
  }

  .grid {
    display: grid;
    grid-template-columns: 30px repeat(var(--weeks), var(--cell));
    grid-auto-rows: var(--cell);
    gap: 2px;
    overflow-x: auto;
  }

  .months {
    display: contents;
  }

  .month {
    grid-row: 1;
    font-size: 10px;
    white-space: nowrap;
    line-height: var(--cell);
  }

  .weekday {
    grid-column: 1;
    font-size: 10px;
    line-height: var(--cell);
  }

  .cell {
    width: var(--cell);
    height: var(--cell);
    padding: 0;
    border: none;
    border-radius: 2px;
    cursor: pointer;
  }

  .cell:hover,
  .cell:focus-visible {
    outline: 2px solid var(--text);
    outline-offset: -1px;
  }

  .level-0 {
    background: var(--seq-0);
    cursor: default;
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
    flex-wrap: wrap;
  }

  .swatch {
    width: 12px;
    height: 12px;
    border-radius: 2px;
  }

  .hint {
    margin-left: auto;
  }
</style>
