<script lang="ts">
  import { num, t } from "$lib/i18n.svelte";
  import { percent } from "$lib/insights/chart";
  import { hideTip, showTip } from "$lib/insights/tooltip.svelte";
  import type { Count } from "$lib/insights/types";

  // Words placed along a spiral from the middle, biggest first, never
  // overlapping. Size follows the count (logarithmically, so a few very
  // common words do not dwarf the rest); all text is horizontal.
  let {
    words,
    total,
    onPick,
    height = 340,
  }: { words: Count[]; total: number; onPick?: (word: string) => void; height?: number } = $props();

  let width = $state(700);
  let hovered = $state<string | null>(null);

  interface Placed {
    word: string;
    count: number;
    x: number;
    y: number;
    size: number;
    rank: number;
  }

  let canvas: HTMLCanvasElement | null = null;

  const placed = $derived.by((): Placed[] => {
    const list = words.slice(0, 90);
    if (list.length === 0 || width < 50) return [];
    canvas ??= document.createElement("canvas");
    const ctx = canvas.getContext("2d");
    if (!ctx) return [];
    const family = getComputedStyle(document.body).fontFamily;
    const low = Math.log(list[list.length - 1].count);
    const high = Math.log(list[0].count);
    const biggest = Math.min(64, Math.max(22, width / 9));
    const smallest = 12;
    const boxes: { x: number; y: number; w: number; h: number }[] = [];
    const result: Placed[] = [];
    const cx = width / 2;
    const cy = height / 2;
    list.forEach((item, rank) => {
      const weight = high > low ? (Math.log(item.count) - low) / (high - low) : 1;
      const size = Math.round(smallest + weight * (biggest - smallest));
      ctx.font = `650 ${size}px ${family}`;
      const w = ctx.measureText(item.key).width + 6;
      const h = size * 1.08;
      for (let step = 0; step < 2600; step++) {
        const angle = step * 0.32;
        const radius = 2.4 * angle;
        const x = cx + radius * Math.cos(angle) * 1.5;
        const y = cy + radius * Math.sin(angle);
        const box = { x: x - w / 2, y: y - h / 2, w, h };
        if (box.x < 0 || box.y < 0 || box.x + w > width || box.y + h > height) continue;
        const overlaps = boxes.some(
          (b) => box.x < b.x + b.w && box.x + box.w > b.x && box.y < b.y + b.h && box.y + box.h > b.y,
        );
        if (overlaps) continue;
        boxes.push(box);
        result.push({ word: item.key, count: item.count, x, y, size, rank });
        break;
      }
    });
    return result;
  });
</script>

<div class="cloud" bind:clientWidth={width}>
  <svg {width} {height} role="img" aria-label={t("insights.wordCloud")}>
    {#each placed as p (p.word)}
      <text
        x={p.x}
        y={p.y}
        font-size={p.size}
        class:top={p.rank < 8}
        class:hovered={hovered === p.word}
        class:pickable={!!onPick}
        text-anchor="middle"
        dominant-baseline="central"
        role="presentation"
        onpointermove={(e) => {
          hovered = p.word;
          showTip(e, p.word, [
            { label: t("insights.times"), value: num(p.count) },
            { label: t("insights.ofAllWords"), value: percent(p.count / Math.max(1, total)) },
          ]);
        }}
        onpointerleave={() => ((hovered = null), hideTip())}
        onclick={() => {
          hideTip();
          onPick?.(p.word);
        }}>{p.word}</text
      >
    {/each}
  </svg>
</div>

<style>
  .cloud {
    width: 100%;
    min-width: 0;
  }

  svg {
    display: block;
  }

  text {
    font-weight: 650;
    fill: var(--text);
    opacity: 0.82;
    transition: opacity 0.1s;
  }

  text.top {
    fill: var(--accent);
    opacity: 1;
  }

  text.pickable {
    cursor: pointer;
  }

  text.hovered {
    opacity: 1;
    text-decoration: underline;
  }
</style>
