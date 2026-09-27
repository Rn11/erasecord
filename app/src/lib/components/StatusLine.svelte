<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { t } from "$lib/i18n.svelte";
  import { activityText, secondsLeft, waitingText, type Waiting } from "$lib/status";
  import type { Activity } from "$lib/types";

  // One line under a title: what is happening, or what EraseCord waits for.
  let {
    activity,
    waiting,
    placeName,
    busy = true,
    paused = false,
  }: {
    activity: Activity | null;
    waiting: Waiting | null;
    placeName: (id: string) => string;
    busy?: boolean;
    paused?: boolean;
  } = $props();

  let now = $state(Date.now());
  let timer: ReturnType<typeof setInterval> | undefined;
  onMount(() => (timer = setInterval(() => (now = Date.now()), 500)));
  onDestroy(() => clearInterval(timer));

  const left = $derived(secondsLeft(waiting, now));
  const text = $derived.by(() => {
    if (paused) return t("status.paused");
    if (waiting && left !== null) return waitingText(waiting, left);
    if (activity) return activityText(activity, placeName);
    return t("status.starting");
  });
</script>

{#if busy}
  <p class="status small" role="status" aria-live="polite" class:waiting={left !== null && !paused}>
    {#if !paused}<span class="dot" aria-hidden="true"></span>{/if}
    <span>{text}</span>
  </p>
{/if}

<style>
  .status {
    margin: 0;
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--muted);
    min-height: 1.4em;
  }

  .waiting {
    color: var(--warn);
  }

  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: currentColor;
    flex: none;
    animation: pulse 1.2s ease-in-out infinite;
  }

  @keyframes pulse {
    50% {
      opacity: 0.25;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .dot {
      animation: none;
    }
  }
</style>
