<script lang="ts">
  import { num, t } from "$lib/i18n.svelte";
  import { addDays, isoDay } from "$lib/insights/chart";
  import { placeName, scopeIsEmpty } from "$lib/insights/store.svelte";
  import { emptyScope, type Info, type Scope } from "$lib/insights/types";
  import type { PackageSummary } from "$lib/types";
  import Avatar from "../Avatar.svelte";

  let { scope = $bindable(), info, pkg }: { scope: Scope; info: Info; pkg: PackageSummary | null } = $props();

  const today = isoDay(new Date());
  const years = $derived.by(() => {
    const first = Number(info.first_day?.slice(0, 4) ?? today.slice(0, 4));
    const last = Number(info.last_day?.slice(0, 4) ?? today.slice(0, 4));
    return Array.from({ length: last - first + 1 }, (_, i) => last - i);
  });

  /** The period menu's choice, derived from the scope. */
  const period = $derived.by(() => {
    if (!scope.from && !scope.to) return "all";
    if (scope.to === today && scope.from === addDays(today, -29)) return "30d";
    if (scope.to === today && scope.from === addDays(today, -364)) return "12m";
    const y = scope.from?.slice(0, 4);
    if (y && scope.from === `${y}-01-01` && scope.to === `${y}-12-31`) return `y${y}`;
    return "custom";
  });
  let custom = $state(false);
  const showDates = $derived(custom || period === "custom");

  function choosePeriod(value: string) {
    custom = value === "custom";
    if (value === "all") Object.assign(scope, { from: null, to: null });
    else if (value === "30d") Object.assign(scope, { from: addDays(today, -29), to: today });
    else if (value === "12m") Object.assign(scope, { from: addDays(today, -364), to: today });
    else if (value.startsWith("y")) Object.assign(scope, { from: `${value.slice(1)}-01-01`, to: `${value.slice(1)}-12-31` });
    else if (!scope.from && !scope.to) Object.assign(scope, { from: info.first_day, to: info.last_day });
  }

  let open = $state(false);
  let query = $state("");
  let picker = $state<HTMLElement>();
  const places = $derived(
    info.places
      .map((p, i) => ({ ...p, index: i, label: placeName(i, pkg) }))
      .filter((p) => p.label.toLowerCase().includes(query.trim().toLowerCase())),
  );
  const placesLabel = $derived(
    scope.places.length === 0
      ? t("insights.allPlaces")
      : scope.places.length === 1
        ? placeName(
            info.places.findIndex((p) => p.id === scope.places[0]),
            pkg,
          )
        : t("count.place", { count: scope.places.length }),
  );

  function togglePlace(id: string) {
    scope.channels = [];
    scope.places = scope.places.includes(id) ? scope.places.filter((p) => p !== id) : [...scope.places, id];
  }

  function onWindowClick(event: MouseEvent) {
    if (open && picker && !picker.contains(event.target as Node)) open = false;
  }
</script>

<svelte:window onclick={onWindowClick} onkeydown={(e) => e.key === "Escape" && (open = false)} />

<div class="scope small" role="group" aria-label={t("insights.filters")}>
  <label class="field">
    <span class="muted">{t("insights.period")}</span>
    <select value={showDates ? "custom" : period} onchange={(e) => choosePeriod(e.currentTarget.value)}>
      <option value="all">{t("insights.allTime")}</option>
      <option value="30d">{t("insights.last30")}</option>
      <option value="12m">{t("insights.last12m")}</option>
      {#each years as year (year)}<option value="y{year}">{year}</option>{/each}
      <option value="custom">{t("insights.customRange")}</option>
    </select>
  </label>
  {#if showDates}
    <label class="field">
      <span class="muted">{t("setup.from")}</span>
      <input type="date" value={scope.from ?? ""} onchange={(e) => (scope.from = e.currentTarget.value || null)} />
    </label>
    <label class="field">
      <span class="muted">{t("setup.to")}</span>
      <input type="date" value={scope.to ?? ""} onchange={(e) => (scope.to = e.currentTarget.value || null)} />
    </label>
  {/if}

  <div class="picker" bind:this={picker}>
    <span class="muted">{t("insights.where")}</span>
    <button class="btn small choose" aria-expanded={open} onclick={() => (open = !open)}>
      <span class="choose-label">{placesLabel}</span> ▾
    </button>
    {#if open}
      <div class="popover card">
        <input type="search" placeholder={t("setup.filterByName")} bind:value={query} />
        <div class="list">
          {#each places as place (place.id)}
            <label class="row" class:checked={scope.places.includes(place.id)}>
              <input type="checkbox" checked={scope.places.includes(place.id)} onchange={() => togglePlace(place.id)} />
              <Avatar name={place.label} url={null} size={20} />
              <span class="name">{place.label}</span>
              <span class="muted num">{num(place.messages)}</span>
            </label>
          {/each}
        </div>
        {#if scope.places.length > 0}
          <button class="link small" onclick={() => ((scope.places = []), (scope.channels = []))}>{t("insights.allPlaces")}</button>
        {/if}
      </div>
    {/if}
  </div>

  {#if scope.channels.length > 0}
    <span class="chip">
      {t("count.channel", { count: scope.channels.length })}
      <button class="link" aria-label={t("common.dismiss")} onclick={() => (scope.channels = [])}>✕</button>
    </span>
  {/if}

  {#if !scopeIsEmpty(scope)}
    <button class="link small reset" onclick={() => ((custom = false), Object.assign(scope, emptyScope()))}>
      {t("insights.resetFilters")}
    </button>
  {/if}
</div>

<style>
  .scope {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    gap: 10px 14px;
  }

  .field,
  .picker {
    display: grid;
    gap: 3px;
  }

  .picker {
    position: relative;
  }

  .choose {
    max-width: 260px;
    display: flex;
    gap: 6px;
  }

  .choose-label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .popover {
    position: absolute;
    top: calc(100% + 6px);
    left: 0;
    z-index: 20;
    width: min(360px, 80vw);
    padding: 10px;
    display: grid;
    gap: 8px;
    box-shadow: var(--shadow);
  }

  .list {
    max-height: 320px;
    overflow-y: auto;
    display: grid;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 5px 6px;
    border-radius: 6px;
    cursor: pointer;
  }

  .row:hover,
  .row.checked {
    background: var(--accent-soft);
  }

  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 4px 10px;
    border-radius: 999px;
    background: var(--accent-soft);
  }

  .link {
    background: none;
    border: none;
    padding: 0;
    font: inherit;
    color: var(--accent);
    cursor: pointer;
  }

  .reset {
    padding-bottom: 6px;
  }
</style>
