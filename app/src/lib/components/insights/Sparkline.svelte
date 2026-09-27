<script lang="ts">
  // A small line of values over time, scaled to its own maximum: for
  // comparing the shape of activity, not the amounts.
  let { values, width = 120, height = 26 }: { values: number[]; width?: number; height?: number } = $props();

  const path = $derived.by(() => {
    if (values.length === 0) return { line: "", area: "" };
    const max = Math.max(1, ...values);
    const step = values.length > 1 ? width / (values.length - 1) : 0;
    const points = values.map((v, i) => [values.length > 1 ? i * step : width / 2, height - 2 - (v / max) * (height - 4)]);
    const line = points.map(([x, y], i) => `${i ? "L" : "M"}${x.toFixed(1)},${y.toFixed(1)}`).join("");
    const area = `${line}L${points[points.length - 1][0].toFixed(1)},${height}L${points[0][0].toFixed(1)},${height}Z`;
    return { line, area };
  });
</script>

<svg {width} {height} aria-hidden="true">
  <path d={path.area} class="area" />
  <path d={path.line} class="line" />
</svg>

<style>
  svg {
    display: block;
    overflow: visible;
  }

  .area {
    fill: var(--accent);
    opacity: 0.12;
  }

  .line {
    fill: none;
    stroke: var(--accent);
    stroke-width: 1.5;
    stroke-linejoin: round;
    stroke-linecap: round;
  }
</style>
