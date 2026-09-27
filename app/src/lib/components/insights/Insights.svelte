<script lang="ts">
  import { num, t } from "$lib/i18n.svelte";
  import { insights, loadInfo } from "$lib/insights/store.svelte";
  import type { CleanUpRequest, Section } from "$lib/insights/types";
  import type { LastPackage } from "$lib/lastPackage";
  import type { PackageSummary } from "$lib/types";
  import PackageGuide from "../PackageGuide.svelte";
  import LinksView from "./LinksView.svelte";
  import Overview from "./Overview.svelte";
  import PlacesView from "./PlacesView.svelte";
  import ScopeBar from "./ScopeBar.svelte";
  import SearchView from "./SearchView.svelte";
  import TimeView from "./TimeView.svelte";
  import Tooltip from "./Tooltip.svelte";
  import WordsView from "./WordsView.svelte";

  let {
    pkg,
    importing,
    importError,
    onImport,
    lastPackage,
    onReopen,
    onForget,
    onCleanUp,
  }: {
    pkg: PackageSummary | null;
    importing: boolean;
    importError: string | null;
    onImport: (folder: boolean) => void;
    lastPackage: LastPackage | null;
    onReopen: () => void;
    onForget: () => void;
    /** Opens Clean up with these messages selected. */
    onCleanUp: (request: CleanUpRequest) => void;
  } = $props();

  function search(words: string) {
    insights.query = words;
    insights.section = "search";
  }

  $effect(() => {
    if (pkg && !insights.info) loadInfo();
  });

  const sections: { id: Section; label: () => string }[] = [
    { id: "overview", label: () => t("insights.overview") },
    { id: "time", label: () => t("insights.time") },
    { id: "places", label: () => t("insights.places") },
    { id: "words", label: () => t("insights.words") },
    { id: "links", label: () => t("insights.linksFiles") },
    { id: "search", label: () => t("insights.search") },
  ];
</script>

<Tooltip />

<section class="card insights">
  {#if !pkg}
    <div class="empty">
      <h2>{t("insights.title")}</h2>
      <p>{t("insights.intro")}</p>
      <p class="muted small">{t("insights.privacy")}</p>
      {#if importing}
        <p class="muted"><span class="spinner"></span> {t("setup.readingPackage")}</p>
      {:else}
        <div class="actions">
          <button class="btn primary" onclick={() => onImport(false)}>{t("setup.importPackage")}</button>
          <button class="btn" onclick={() => onImport(true)} title={t("setup.importFolderHint")}>
            {t("insights.importFolder")}
          </button>
        </div>
      {/if}
      {#if lastPackage && !importing}
        <p class="small last">
          <button class="link" onclick={onReopen}>{t("insights.reopen", { name: lastPackage.name })}</button>
          <span class="muted">· {t("count.message", { count: lastPackage.messages })}</span>
          <button class="link muted" onclick={onForget}>{t("insights.forget")}</button>
        </p>
      {/if}
      <p class="muted small">{t("insights.dropHint")}</p>
      {#if importError}<p class="callout error small">{importError}</p>{/if}
      <PackageGuide open={!lastPackage} />
    </div>
  {:else if insights.infoError}
    <p class="callout error small">{insights.infoError}</p>
  {:else if !insights.info}
    <p class="muted"><span class="spinner"></span> {t("stats.loading")}</p>
  {:else}
    <header>
      <div class="heading">
        <h2>{t("insights.title")}</h2>
        <span class="muted small">
          {t("setup.packageSummary", {
            messages: t("count.message", { count: insights.info.messages }),
            places: t("count.place", { count: insights.info.places.length }),
          })}
        </span>
      </div>
      <ScopeBar bind:scope={insights.scope} info={insights.info} {pkg} />
    </header>

    <nav class="sections" aria-label={t("insights.sections")}>
      {#each sections as section (section.id)}
        <button
          class:active={insights.section === section.id}
          aria-current={insights.section === section.id ? "page" : undefined}
          onclick={() => (insights.section = section.id)}
        >
          {section.label()}
        </button>
      {/each}
    </nav>

    <div class="content">
      {#if insights.section === "overview"}
        <Overview bind:scope={insights.scope} {pkg} />
      {:else if insights.section === "time"}
        <TimeView bind:scope={insights.scope} {pkg} />
      {:else if insights.section === "places"}
        <PlacesView bind:scope={insights.scope} {pkg} {onCleanUp} />
      {:else if insights.section === "words"}
        <WordsView bind:scope={insights.scope} {pkg} onSearch={search} />
      {:else if insights.section === "links"}
        <LinksView bind:scope={insights.scope} />
      {:else if insights.section === "search"}
        <SearchView bind:scope={insights.scope} {pkg} {onCleanUp} />
      {/if}
    </div>
    <p class="muted small privacy">{t("insights.privacyShort", { count: num(insights.info.messages) })}</p>
  {/if}
</section>

<style>
  .insights {
    width: 100%;
    max-width: 1080px;
    margin: 0 auto;
    padding: 20px 22px;
    display: grid;
    gap: 16px;
    align-content: start;
    overflow-y: auto;
    max-height: 100%;
  }

  .empty {
    max-width: 560px;
    margin: 40px auto;
    display: grid;
    gap: 10px;
    text-align: center;
    justify-items: center;
  }

  .empty h2 {
    margin: 0;
  }

  .empty p {
    margin: 0;
  }

  .last {
    display: flex;
    gap: 6px;
    align-items: baseline;
    flex-wrap: wrap;
    justify-content: center;
  }

  .link {
    background: none;
    border: none;
    padding: 0;
    font: inherit;
    color: var(--accent);
    cursor: pointer;
  }

  .link.muted {
    color: var(--muted);
    text-decoration: underline;
  }

  .actions {
    display: flex;
    gap: 8px;
    margin-top: 8px;
  }

  header {
    display: grid;
    gap: 12px;
  }

  .heading {
    display: flex;
    align-items: baseline;
    gap: 12px;
    flex-wrap: wrap;
  }

  h2 {
    margin: 0;
  }

  .sections {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    border-bottom: 1px solid var(--border);
  }

  .sections button {
    border: none;
    background: none;
    font: inherit;
    font-weight: 550;
    color: var(--muted);
    padding: 8px 12px;
    border-bottom: 2px solid transparent;
    margin-bottom: -1px;
    cursor: pointer;
    white-space: nowrap;
  }

  .sections button.active {
    color: var(--text);
    border-bottom-color: var(--accent);
  }

  .content {
    min-width: 0;
  }

  .privacy {
    margin: 0;
    text-align: center;
  }
</style>
