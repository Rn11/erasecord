<script lang="ts">
  import { api } from "$lib/api";
  import { num, t, type Key } from "$lib/i18n.svelte";
  import { formatDate, localDate, monthName, OTHER_COLOR, percent, SERIES_COLORS } from "$lib/insights/chart";
  import { useQuery } from "$lib/insights/query.svelte";
  import { hideTip, showTip } from "$lib/insights/tooltip.svelte";
  import { FILE_KINDS, type Bucket, type Scope } from "$lib/insights/types";
  import StackedColumns from "./StackedColumns.svelte";

  let { scope = $bindable() }: { scope: Scope } = $props();

  const q = useQuery("links", () => scope, api.insightsLinks);
  const data = $derived(q.data);
  const domainMax = $derived(Math.max(1, ...(data?.domains ?? []).map((d) => d.count)));
  const kindLabel = (kind: string) => t(`insights.kind.${kind}` as Key);

  // Stacked per month: the first five kinds in colour, "other" as the rest.
  const columns = $derived<Bucket[]>(
    (data?.months ?? []).map((key, i) => {
      const row = data?.monthly[i] ?? [];
      return { key, total: row.reduce((a, b) => a + b, 0), series: row.slice(0, FILE_KINDS.length - 1) };
    }),
  );
  const series = $derived(FILE_KINDS.slice(0, -1).map((k, i) => ({ label: kindLabel(k), color: SERIES_COLORS[i] })));
  const kindColor = (i: number) => (i < FILE_KINDS.length - 1 ? SERIES_COLORS[i] : OTHER_COLOR);

  function axisLabel(key: string, index: number): string | null {
    const date = localDate(key);
    if (columns.length <= 14) return formatDate(date, { month: "short" });
    return date.getMonth() === 0 || index === 0 ? String(date.getFullYear()) : null;
  }
</script>

{#if q.error}
  <p class="callout error small">{q.error}</p>
{:else if !data}
  <p class="muted"><span class="spinner"></span> {t("stats.loading")}</p>
{:else if data.links === 0 && data.attachments === 0}
  <p class="muted">{t("insights.noLinks")}</p>
{:else}
  <div class="links" class:stale={q.loading}>
    <div class="tiles">
      <div class="tile">
        <span class="label">{t("insights.links")}</span>
        <span class="value">{num(data.links)}</span>
        <span class="detail">{t("stats.inMessages", { count: data.messages_with_links })}</span>
      </div>
      <div class="tile">
        <span class="label">{t("insights.websites")}</span>
        <span class="value">{num(data.domains.length)}{data.domains.length >= 40 ? "+" : ""}</span>
      </div>
      <div class="tile">
        <span class="label">{t("stats.attachments")}</span>
        <span class="value">{num(data.attachments)}</span>
      </div>
    </div>

    {#if data.domains.length > 0}
      <figure>
        <figcaption><span>{t("insights.topSites")}</span></figcaption>
        <div class="bars">
          {#each data.domains.slice(0, 20) as d (d.key)}
            <div
              class="bar-row"
              role="presentation"
              onpointermove={(e) =>
                showTip(e, d.key, [
                  { label: t("insights.links"), value: num(d.count) },
                  { label: t("insights.share"), value: percent(d.count / Math.max(1, data.links)) },
                ])}
              onpointerleave={hideTip}
            >
              <span class="name small">{d.key}</span>
              <span class="track">
                <span class="bar" style:width="{(d.count / domainMax) * 100}%"></span>
                <span class="value num small">{num(d.count)}</span>
              </span>
            </div>
          {/each}
        </div>
      </figure>
    {/if}

    {#if data.attachments > 0}
      <figure>
        <figcaption><span>{t("insights.fileKinds")}</span></figcaption>
        <div class="kinds small">
          {#each data.kinds as kind, i (kind.key)}
            {#if kind.count > 0}
              <span class="kind">
                <span class="swatch" style:background={kindColor(i)}></span>
                {kindLabel(kind.key)}
                <strong class="num">{num(kind.count)}</strong>
                <span class="muted">{percent(kind.count / Math.max(1, data.attachments))}</span>
              </span>
            {/if}
          {/each}
        </div>
        {#if columns.length > 1}
          <StackedColumns {columns} {series} {axisLabel} title={(key) => monthName(key)} height={180} legend={false} />
        {/if}
        {#if data.extensions.length > 0}
          <p class="muted small">
            {t("insights.fileTypes")}
            {data.extensions
              .slice(0, 12)
              .map((e) => `.${e.key} ${num(e.count)}`)
              .join(" · ")}
          </p>
        {/if}
      </figure>
    {/if}
  </div>
{/if}

<style>
  .links {
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

  .label,
  .detail {
    font-size: 13px;
    color: var(--muted);
  }

  .detail {
    font-size: 12px;
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
    font-weight: 600;
  }

  figure p {
    margin: 0;
  }

  .bars {
    display: grid;
    gap: 3px;
  }

  .bar-row {
    display: grid;
    grid-template-columns: minmax(120px, 240px) minmax(0, 1fr);
    gap: 12px;
    align-items: center;
  }

  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .track {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .bar {
    height: 14px;
    min-width: 2px;
    max-width: calc(100% - 64px);
    background: var(--accent);
    border-radius: 0 4px 4px 0;
  }

  .kinds {
    display: flex;
    flex-wrap: wrap;
    gap: 6px 18px;
  }

  .kind {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }

  .swatch {
    width: 10px;
    height: 10px;
    border-radius: 3px;
  }
</style>
