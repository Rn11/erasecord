<script lang="ts">
  let { name, url = null, size = 32 }: { name: string; url?: string | null; size?: number } = $props();

  let failed = $state(false);
  const initials = $derived(
    name
      .split(/[\s,–-]+/)
      .filter(Boolean)
      .slice(0, 2)
      .map((word) => [...word][0].toUpperCase())
      .join(""),
  );
  const hue = $derived([...name].reduce((h, c) => (h * 31 + c.charCodeAt(0)) % 360, 7));
</script>

{#if url && !failed}
  <img class="avatar" src={url} alt="" width={size} height={size} onerror={() => (failed = true)} />
{:else}
  <span
    class="avatar initials"
    style:width="{size}px"
    style:height="{size}px"
    style:font-size="{Math.round(size * 0.38)}px"
    style:--hue={hue}
    aria-hidden="true">{initials}</span
  >
{/if}

<style>
  .avatar {
    border-radius: 50%;
    flex: none;
    object-fit: cover;
  }

  .initials {
    display: inline-grid;
    place-items: center;
    background: hsl(var(--hue) 42% 52%);
    color: #fff;
    font-weight: 650;
    letter-spacing: 0.02em;
  }
</style>
