<script lang="ts">
  import { tip } from "$lib/insights/tooltip.svelte";

  let width = $state(0);
  let height = $state(0);
  // Above the pointer, kept inside the window.
  const left = $derived(Math.max(8, Math.min(tip.x - width / 2, window.innerWidth - width - 8)));
  const top = $derived(tip.y - height - 14 < 8 ? tip.y + 18 : tip.y - height - 14);
</script>

{#if tip.visible}
  <div class="tooltip small" style:left="{left}px" style:top="{top}px" role="status" bind:clientWidth={width} bind:clientHeight={height}>
    <strong>{tip.title}</strong>
    {#each tip.lines as line, i (i)}
      <span class="line">
        {#if line.color}<span class="swatch" style:background={line.color}></span>{/if}
        <span class="label">{line.label}</span>
        <span class="value num">{line.value}</span>
      </span>
    {/each}
  </div>
{/if}

<style>
  .tooltip {
    position: fixed;
    z-index: 50;
    pointer-events: none;
    background: var(--panel);
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow: var(--shadow);
    padding: 7px 10px;
    display: grid;
    gap: 3px;
    max-width: 320px;
  }

  .line {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .label {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--muted);
  }

  .value {
    font-weight: 600;
  }

  .swatch {
    width: 10px;
    height: 10px;
    border-radius: 3px;
    flex: none;
  }
</style>
