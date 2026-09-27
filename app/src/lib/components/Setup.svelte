<script lang="ts">
  import { SvelteSet, type SvelteMap } from "svelte/reactivity";
  import { open } from "@tauri-apps/plugin-dialog";
  import {
    HAS_KINDS,
    contentProblem,
    describePlaces,
    filterLines,
    hasLabel,
    rangeProblem,
    toFilter,
    type ContentForm,
    type RangeForm,
  } from "$lib/format";
  import { num, t } from "$lib/i18n.svelte";
  import { loadPresets, savePresets, snapshot, withPreset, type Preset } from "$lib/presets";
  import type { Friend, GuildChannel, Has, JobOptions, PackageSummary, Target } from "$lib/types";
  import Avatar from "./Avatar.svelte";

  let {
    targets,
    pkg,
    importing,
    importError,
    onImport,
    onClosePackage,
    loading,
    error,
    selected,
    channelPicks,
    channelLists,
    onLoadChannels,
    friends,
    friendsLoading,
    friendsError,
    opening,
    onLoadFriends,
    onOpenFriend,
    range = $bindable(),
    content = $bindable(),
    skipPinned = $bindable(),
    options = $bindable(),
    onReload,
    onCount,
  }: {
    targets: Target[];
    pkg: PackageSummary | null;
    importing: boolean;
    importError: string | null;
    onImport: (folder: boolean) => void;
    onClosePackage: () => void;
    loading: boolean;
    error: string | null;
    selected: SvelteSet<string>;
    channelPicks: SvelteMap<string, string[]>;
    channelLists: SvelteMap<string, { loading: boolean; error: string | null; channels: GuildChannel[] }>;
    onLoadChannels: (guildId: string) => void;
    friends: Friend[] | null;
    friendsLoading: boolean;
    friendsError: string | null;
    opening: SvelteSet<string>;
    onLoadFriends: () => void;
    onOpenFriend: (friend: Friend) => void;
    range: RangeForm;
    content: ContentForm;
    skipPinned: boolean;
    options: JobOptions;
    onReload: () => void;
    onCount: () => void;
  } = $props();

  let tab = $state<"servers" | "dms">("servers");
  let query = $state("");

  const servers = $derived(targets.filter((t) => t.kind === "guild"));
  const dms = $derived(targets.filter((t) => t.kind !== "guild"));
  const visible = $derived(
    (tab === "servers" ? servers : dms).filter((t) => t.name.toLowerCase().includes(query.trim().toLowerCase())),
  );
  const visibleFriends = $derived(
    (friends ?? []).filter((f) => f.name.toLowerCase().includes(query.trim().toLowerCase())),
  );
  const allVisibleSelected = $derived(visible.length > 0 && visible.every((t) => selected.has(t.id)));
  const selectedServers = $derived(servers.filter((t) => selected.has(t.id)).length);
  const selectedDms = $derived(dms.filter((t) => selected.has(t.id)).length);
  const problem = $derived(rangeProblem(range) ?? contentProblem(content));
  const summary = $derived(problem ? [] : filterLines(toFilter(range, skipPinned, content)));
  const contentActive = $derived(
    !!content.contains.trim() || !!content.pattern.trim() || content.has.length > 0 || content.without.length > 0,
  );

  function toggleKind(list: "has" | "without", kind: Has) {
    const current = content[list];
    content[list] = current.includes(kind) ? current.filter((k) => k !== kind) : [...current, kind];
  }

  const expanded = new SvelteSet<string>();
  const packageCounts = $derived(new Map((pkg?.targets ?? []).map((t) => [t.target.id, t.messages])));
  const left = $derived(new Set(pkg?.left_servers ?? []));
  // The data package cannot tell embeds and stickers apart.
  const kinds = $derived(pkg ? HAS_KINDS.filter((k) => k !== "embed" && k !== "sticker") : HAS_KINDS);

  function toggle(id: string) {
    if (selected.has(id)) {
      selected.delete(id);
      channelPicks.delete(id);
    } else selected.add(id);
  }

  // Lists are dropped on reload or when switching to or from a package;
  // fetch them again for servers that are still expanded.
  $effect(() => {
    for (const id of expanded) if (!channelLists.has(id)) onLoadChannels(id);
  });

  async function chooseBackupFolder(event?: Event) {
    const checkbox = event?.currentTarget as HTMLInputElement | undefined;
    if (checkbox && !checkbox.checked) {
      options.backup_dir = null;
      return;
    }
    let folder: string | string[] | null = null;
    try {
      folder = await open({ directory: true, multiple: false, title: t("setup.backupPick") });
    } catch {
      folder = null;
    }
    if (typeof folder === "string") options.backup_dir = folder;
    else if (checkbox) checkbox.checked = options.backup_dir !== null;
  }

  let presets = $state<Preset[]>(loadPresets());
  let preset = $state("");
  /** The name being typed while saving a preset; null when not saving. */
  let presetName = $state<string | null>(null);
  let presetNote = $state<string | null>(null);

  function applyPreset() {
    const chosen = presets.find((p) => p.name === preset);
    presetNote = null;
    if (!chosen) return;
    const settings = $state.snapshot(chosen.settings);
    range = settings.range;
    content = settings.content;
    skipPinned = settings.skipPinned;
    options = { ...settings.options, dry_run: options.dry_run };
    if (chosen.targets.length === 0) return;
    const known = new Set(targets.map((t) => t.id));
    selected.clear();
    channelPicks.clear();
    for (const id of chosen.targets) if (known.has(id)) selected.add(id);
    for (const [id, channels] of Object.entries(chosen.channels))
      if (selected.has(id) && channels.length > 0) channelPicks.set(id, [...channels]);
    const missing = chosen.targets.length - selected.size;
    if (missing > 0) presetNote = t("presets.missing", { count: missing });
  }

  function storePreset(event: SubmitEvent) {
    event.preventDefault();
    const name = presetName?.trim();
    if (!name) return;
    presets = withPreset(presets, {
      name,
      settings: snapshot($state.snapshot(range), $state.snapshot(content), skipPinned, $state.snapshot(options)),
      targets: [...selected],
      channels: Object.fromEntries([...channelPicks].filter(([id]) => selected.has(id))),
    });
    savePresets(presets);
    preset = presets.find((p) => p.name.toLocaleLowerCase() === name.toLocaleLowerCase())?.name ?? "";
    presetName = null;
    presetNote = null;
  }

  function deletePreset() {
    presets = presets.filter((p) => p.name !== preset);
    savePresets(presets);
    preset = "";
    presetNote = null;
  }

  function toggleExpanded(id: string) {
    if (expanded.has(id)) expanded.delete(id);
    else {
      expanded.add(id);
      onLoadChannels(id);
    }
  }

  function toggleChannel(guildId: string, channelId: string) {
    const picks = channelPicks.get(guildId) ?? [];
    const next = picks.includes(channelId) ? picks.filter((id) => id !== channelId) : [...picks, channelId];
    if (next.length) {
      channelPicks.set(guildId, next);
      selected.add(guildId);
    } else {
      // Never widen to the whole server behind the user's back.
      channelPicks.delete(guildId);
      selected.delete(guildId);
    }
  }

  /** Channels grouped under their category, keeping Discord's order. */
  function grouped(channels: GuildChannel[]): { category: string | null; channels: GuildChannel[] }[] {
    const groups: { category: string | null; channels: GuildChannel[] }[] = [];
    for (const channel of channels) {
      const last = groups[groups.length - 1];
      if (last && last.category === channel.category) last.channels.push(channel);
      else groups.push({ category: channel.category, channels: [channel] });
    }
    return groups;
  }

  const channelIcon = (kind: number) => (kind === 2 || kind === 13 ? "🔊" : kind === 5 ? "📢" : "#");

  function toggleVisible() {
    const select = !allVisibleSelected;
    for (const target of visible) {
      if (select) selected.add(target.id);
      else {
        selected.delete(target.id);
        channelPicks.delete(target.id);
      }
    }
  }
</script>

<div class="setup">
  <section class="card picker" aria-label={t("setup.pickerLabel")}>
    <div class="source small" class:package={pkg}>
      {#if pkg}
        <span>
          <strong>{t("setup.dataPackage")}</strong> ·
          {t("setup.packageSummary", {
            messages: t("count.message", { count: pkg.messages }),
            places: t("count.place", { count: pkg.targets.length }),
          })}
        </span>
        <button class="link" onclick={onClosePackage}>{t("setup.backToLive")}</button>
      {:else if importing}
        <span class="muted"><span class="spinner"></span> {t("setup.readingPackage")}</span>
      {:else}
        <span class="muted">{t("setup.liveSource")}</span>
        <span class="import">
          <button class="link" onclick={() => onImport(false)}>{t("setup.importPackage")}</button>
          <button class="link muted" onclick={() => onImport(true)} title={t("setup.importFolderHint")}>
            {t("setup.importFolder")}
          </button>
        </span>
      {/if}
    </div>
    {#if importError}<p class="small problem source-error">{importError}</p>{/if}
    <div class="tabs" role="tablist">
      <button role="tab" aria-selected={tab === "servers"} class:active={tab === "servers"} onclick={() => (tab = "servers")}>
        {t("setup.tabServers")} <span class="count">{selectedServers ? `${num(selectedServers)}/` : ""}{num(servers.length)}</span>
      </button>
      <button role="tab" aria-selected={tab === "dms"} class:active={tab === "dms"} onclick={() => (tab = "dms")}>
        {t("setup.tabDms")} <span class="count">{selectedDms ? `${num(selectedDms)}/` : ""}{num(dms.length)}</span>
      </button>
    </div>

    <div class="toolbar">
      <input type="search" placeholder={t("setup.filterByName")} bind:value={query} aria-label={t("setup.filterByName")} />
      <button class="btn small" onclick={toggleVisible} disabled={visible.length === 0}>
        {allVisibleSelected ? t("setup.selectNone") : t("setup.selectAll")}
      </button>
      {#if !pkg}
        <button class="btn small ghost" onclick={onReload} disabled={loading} title={t("setup.reload")} aria-label={t("setup.reload")}>↻</button>
      {/if}
    </div>

    <div class="list">
      {#if loading && targets.length === 0}
        <p class="empty muted"><span class="spinner"></span> {t("setup.loadingTargets")}</p>
      {:else if error}
        <div class="empty">
          <p class="callout error small">{error}</p>
          <button class="btn small" onclick={onReload}>{t("common.tryAgain")}</button>
        </div>
      {:else if visible.length === 0}
        <p class="empty muted">
          {query ? t("setup.noMatch") : tab === "servers" ? t("setup.noServers") : t("setup.noDms")}
        </p>
      {:else}
        {#each visible as target (target.id)}
          {@const picks = channelPicks.get(target.id) ?? []}
          <label class="row" class:checked={selected.has(target.id)}>
            <input type="checkbox" checked={selected.has(target.id)} onchange={() => toggle(target.id)} />
            <Avatar name={target.name} url={target.icon_url} size={28} />
            <span class="name">{target.name}</span>
            {#if target.kind === "group_dm"}<span class="tag">{t("setup.tagGroup")}</span>{/if}
            {#if left.has(target.id)}<span class="tag warn" title={t("setup.leftHint")}>{t("setup.tagLeft")}</span>{/if}
            {#if pkg}<span class="muted small num">{num(packageCounts.get(target.id) ?? 0)}</span>{/if}
            {#if picks.length}<span class="tag picked">{t("count.channel", { count: picks.length })}</span>{/if}
            {#if target.kind === "guild"}
              <button
                type="button"
                class="btn ghost small expand"
                aria-expanded={expanded.has(target.id)}
                title={t("setup.channelsHint")}
                onclick={(event) => {
                  event.preventDefault();
                  toggleExpanded(target.id);
                }}
              >
                {t("setup.channels")} <span class="chevron" class:open={expanded.has(target.id)}>▾</span>
              </button>
            {/if}
          </label>
          {#if expanded.has(target.id)}
            {@const list = channelLists.get(target.id)}
            <div class="channels">
              {#if !list || list.loading}
                <p class="small muted"><span class="spinner"></span> {t("setup.loadingChannels")}</p>
              {:else if list.error}
                <p class="small problem">{list.error}</p>
              {:else if list.channels.length === 0}
                <p class="small muted">{t("setup.noChannels")}</p>
              {:else}
                <p class="small muted">{picks.length ? t("setup.onlyTicked") : t("setup.tickChannels")}</p>
                {#each grouped(list.channels) as group, i (i)}
                  {#if group.category}<div class="category small">{group.category}</div>{/if}
                  {#each group.channels as channel (channel.id)}
                    <label class="channel" class:checked={picks.includes(channel.id)}>
                      <input
                        type="checkbox"
                        checked={picks.includes(channel.id)}
                        onchange={() => toggleChannel(target.id, channel.id)}
                      />
                      <span class="icon muted" aria-hidden="true">{channelIcon(channel.kind)}</span>
                      <span class="name">{channel.name}</span>
                      {#if channel.messages !== undefined}<span class="muted small num">{num(channel.messages)}</span>{/if}
                    </label>
                  {/each}
                {/each}
                {#if !pkg}
                  <p class="small muted">{t("setup.threadsNote")}</p>
                {/if}
              {/if}
            </div>
          {/if}
        {/each}
      {/if}
    </div>
    {#if tab === "dms" && pkg}
      <p class="closed small muted">{t("setup.packageClosedDms")}</p>
    {:else if tab === "dms"}
      <div class="closed">
        {#if friends === null}
          <p class="small muted">{t("setup.onlyOpenDms")}</p>
          <button class="btn small" onclick={onLoadFriends} disabled={friendsLoading}>
            {#if friendsLoading}<span class="spinner"></span>{/if} {t("setup.findFriends")}
          </button>
        {:else}
          <div class="closed-head small">
            <strong>{t("setup.friendsTitle")}</strong>
            <span class="muted">{t("setup.friendsHint")}</span>
          </div>
          {#if visibleFriends.length === 0}
            <p class="small muted">{friends.length ? t("setup.noMatch") : t("setup.allFriendsOpen")}</p>
          {:else}
            <div class="friends">
              {#each visibleFriends as friend (friend.user_id)}
                <label class="row">
                  {#if opening.has(friend.user_id)}
                    <span class="spinner muted"></span>
                  {:else}
                    <input type="checkbox" checked={false} onchange={() => onOpenFriend(friend)} />
                  {/if}
                  <Avatar name={friend.name} url={friend.icon_url} size={24} />
                  <span class="name">{friend.name}</span>
                </label>
              {/each}
            </div>
          {/if}
        {/if}
        {#if friendsError}<p class="small problem">{friendsError}</p>{/if}
      </div>
    {/if}
  </section>

  <section class="card options" aria-label={t("setup.whatToDelete")}>
    <h2>{t("setup.whatToDelete")}</h2>

    <div class="presets small">
      {#if presetName === null}
        <div class="preset-row">
          <select aria-label={t("presets.label")} bind:value={preset} onchange={applyPreset}>
            <option value="">{presets.length ? t("presets.choose") : t("presets.none")}</option>
            {#each presets as p (p.name)}<option value={p.name}>{p.name}</option>{/each}
          </select>
          <button type="button" class="link small" onclick={() => (presetName = preset)} title={t("presets.hint")}>
            {t("presets.save")}
          </button>
          {#if preset}
            <button type="button" class="link small" onclick={deletePreset}>{t("presets.delete")}</button>
          {/if}
        </div>
      {:else}
        <form class="preset-row" onsubmit={storePreset}>
          <!-- svelte-ignore a11y_autofocus -->
          <input
            type="text"
            maxlength="60"
            autofocus
            aria-label={t("presets.name")}
            placeholder={t("presets.name")}
            bind:value={presetName}
            onkeydown={(e) => e.key === "Escape" && (presetName = null)}
          />
          <button type="submit" class="btn small" disabled={!presetName.trim()}>{t("presets.store")}</button>
          <button type="button" class="link small" onclick={() => (presetName = null)}>{t("common.cancel")}</button>
        </form>
        <span class="muted">{t("presets.hint")}</span>
      {/if}
      {#if presetNote}<span class="muted">{presetNote}</span>{/if}
    </div>

    <fieldset>
      <legend>{t("setup.timeRange")}</legend>
      <label class="choice">
        <input type="radio" name="range" value="older_than" bind:group={range.mode} />
        <span>{t("setup.olderThan")}</span>
      </label>
      <div class="indent inline" class:disabled={range.mode !== "older_than"}>
        <input
          type="number"
          min="1"
          step="1"
          bind:value={range.amount}
          disabled={range.mode !== "older_than"}
          aria-label={t("setup.amount")}
        />
        <select bind:value={range.unit} disabled={range.mode !== "older_than"} aria-label={t("setup.unit")}>
          <option value="days">{t("setup.days")}</option>
          <option value="weeks">{t("setup.weeks")}</option>
          <option value="months">{t("setup.months")}</option>
          <option value="years">{t("setup.years")}</option>
        </select>
      </div>

      <label class="choice">
        <input type="radio" name="range" value="between" bind:group={range.mode} />
        <span>{t("setup.between")}</span>
      </label>
      <div class="indent dates" class:disabled={range.mode !== "between"}>
        <input type="date" bind:value={range.from} disabled={range.mode !== "between"} aria-label={t("setup.from")} />
        <span class="muted">{t("setup.and")}</span>
        <input type="date" bind:value={range.to} disabled={range.mode !== "between"} aria-label={t("setup.to")} />
      </div>

      <label class="choice">
        <input type="radio" name="range" value="all" bind:group={range.mode} />
        <span>{t("setup.allMessages")}</span>
      </label>
    </fieldset>

    <fieldset>
      <legend>
        {t("setup.content")}
        {#if contentActive}
          <button type="button" class="link small" onclick={() => (content = { contains: "", pattern: "", has: [], without: [] })}>
            {t("setup.reset")}
          </button>
        {/if}
      </legend>
      <label class="field">
        <span class="small muted">{t("setup.containing")}</span>
        <input type="text" placeholder={t("setup.anyText")} bind:value={content.contains} spellcheck="false" />
      </label>
      <div class="field">
        <span class="small muted">{t("setup.onlyWith")}</span>
        <div class="chips" role="group" aria-label={t("setup.onlyWith")}>
          {#each kinds as kind (kind)}
            <label class="chip" class:on={content.has.includes(kind)}>
              <input type="checkbox" checked={content.has.includes(kind)} onchange={() => toggleKind("has", kind)} />
              {hasLabel(kind)}
            </label>
          {/each}
        </div>
      </div>
      <div class="field">
        <span class="small muted">{t("setup.keepWith")}</span>
        <div class="chips" role="group" aria-label={t("setup.keepWith")}>
          {#each kinds as kind (kind)}
            <label class="chip keep" class:on={content.without.includes(kind)}>
              <input type="checkbox" checked={content.without.includes(kind)} onchange={() => toggleKind("without", kind)} />
              {hasLabel(kind)}
            </label>
          {/each}
        </div>
      </div>
      <details open={!!content.pattern}>
        <summary class="small">{t("setup.regex")}</summary>
        <label class="field regex">
          <input
            type="text"
            placeholder={t("setup.regexPlaceholder")}
            bind:value={content.pattern}
            spellcheck="false"
            aria-label={t("setup.regex")}
          />
          <span class="small muted">{t("setup.regexHint")}</span>
        </label>
      </details>
    </fieldset>

    <fieldset>
      <legend>{t("setup.options")}</legend>
      <label class="choice">
        <input type="checkbox" bind:checked={skipPinned} />
        <span>{t("setup.keepPinned")}</span>
      </label>
      <label class="choice">
        <input
          type="checkbox"
          checked={options.overwrite !== null}
          onchange={(event) => (options.overwrite = event.currentTarget.checked ? "" : null)}
        />
        <span>{t("setup.overwrite")}</span>
      </label>
      {#if options.overwrite !== null}
        <div class="indent field">
          <input
            type="text"
            placeholder={t("setup.overwritePlaceholder")}
            bind:value={options.overwrite}
            aria-label={t("setup.overwriteLabel")}
          />
          <span class="small muted">{t("setup.overwriteHint")}</span>
        </div>
      {/if}
      <label class="choice">
        <input type="checkbox" checked={options.backup_dir !== null} onchange={chooseBackupFolder} />
        <span>{t("setup.backup")}</span>
      </label>
      {#if options.backup_dir !== null}
        <div class="indent field">
          <span class="folder">
            <code title={options.backup_dir}>{options.backup_dir}</code>
            <button type="button" class="link small" onclick={() => chooseBackupFolder()}>{t("setup.backupChange")}</button>
          </span>
          <span class="small muted">{t("setup.backupHint")}</span>
        </div>
      {/if}
    </fieldset>

    <details>
      <summary class="small">{t("setup.speed")}</summary>
      <div class="speed small">
        <label>
          <span>{t("setup.deleteDelay")}</span>
          <span class="unit"><input type="number" min="0" step="100" bind:value={options.delete_delay_ms} /> ms</span>
        </label>
        <label>
          <span>{t("setup.searchDelay")}</span>
          <span class="unit"><input type="number" min="0" step="500" bind:value={options.search_delay_ms} /> ms</span>
        </label>
        <p class="muted">{t("setup.speedHint")}</p>
      </div>
    </details>

    <div class="footer">
      {#if problem}
        <p class="small problem">{problem}</p>
      {:else if selected.size === 0}
        <p class="small muted">{t("setup.selectSomething")}</p>
      {:else}
        <ul class="summary small muted">
          {#each summary as line, i (i)}<li>{line}</li>{/each}
          <li>{t("filter.in", { places: describePlaces(selectedServers, selectedDms) })}</li>
        </ul>
      {/if}
      <button class="btn primary" disabled={selected.size === 0 || !!problem} onclick={onCount}>{t("setup.count")}</button>
    </div>
  </section>
</div>

<style>
  .setup {
    display: grid;
    grid-template-columns: minmax(280px, 1fr) minmax(300px, 380px);
    gap: 16px;
    height: 100%;
    min-height: 0;
  }

  @media (max-width: 760px) {
    /* Stack the panels and let the page scroll instead of each panel. */
    .setup {
      grid-template-columns: 1fr;
      height: auto;
      flex: none;
    }

    .list {
      max-height: 45vh;
    }

    .options {
      overflow: visible;
    }
  }

  .picker {
    display: flex;
    flex-direction: column;
    min-height: 0;
    overflow: hidden;
  }

  .tabs {
    display: flex;
    border-bottom: 1px solid var(--border);
  }

  .tabs button {
    flex: 1;
    padding: 12px;
    background: none;
    border: none;
    border-bottom: 2px solid transparent;
    font: inherit;
    font-weight: 600;
    color: var(--muted);
    cursor: pointer;
  }

  .tabs button.active {
    color: var(--text);
    border-bottom-color: var(--accent);
  }

  .count {
    font-weight: 500;
    color: var(--muted);
    margin-left: 4px;
  }

  .toolbar {
    display: flex;
    gap: 8px;
    padding: 10px;
    border-bottom: 1px solid var(--border);
  }

  .toolbar input {
    flex: 1;
  }

  .list {
    flex: 1;
    overflow-y: auto;
    padding: 6px;
    min-height: 200px;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 7px 8px;
    border-radius: 8px;
    cursor: pointer;
  }

  .row:hover {
    background: var(--panel-2);
  }

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

  .tag {
    font-size: 11px;
    color: var(--muted);
    border: 1px solid var(--border);
    border-radius: 999px;
    padding: 0 7px;
    white-space: nowrap;
  }

  .tag.picked {
    color: var(--accent);
    border-color: var(--accent);
  }

  .expand {
    padding: 2px 8px;
  }

  .chevron {
    display: inline-block;
    transition: transform 0.15s ease;
  }

  .chevron.open {
    transform: rotate(180deg);
  }

  .channels {
    margin: 0 8px 6px 46px;
    padding: 6px 0 6px 10px;
    border-left: 2px solid var(--border);
    display: grid;
    gap: 2px;
  }

  .channels > p {
    padding: 2px 6px;
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .category {
    margin-top: 6px;
    padding: 0 6px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.03em;
    font-size: 11px;
    color: var(--muted);
  }

  .channel {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px 6px;
    border-radius: 6px;
    cursor: pointer;
  }

  .channel:hover {
    background: var(--panel-2);
  }

  .channel.checked {
    background: var(--accent-soft);
  }

  .icon {
    width: 16px;
    text-align: center;
    font-size: 12px;
  }

  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 10px;
    padding: 32px 16px;
    text-align: center;
  }

  p.empty {
    flex-direction: row;
  }

  .source {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    flex-wrap: wrap;
    padding: 8px 12px;
    border-bottom: 1px solid var(--border);
    background: var(--panel-2);
  }

  .source.package {
    background: var(--accent-soft);
  }

  .source > span {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .source-error {
    padding: 6px 12px;
    border-bottom: 1px solid var(--border);
  }

  .tag.warn {
    color: var(--warn);
    border-color: var(--warn);
  }

  .closed {
    padding: 8px 12px 10px;
    border-top: 1px solid var(--border);
    display: grid;
    gap: 8px;
    justify-items: start;
  }

  .closed-head {
    display: grid;
    gap: 2px;
  }

  .friends {
    width: 100%;
    max-height: 30vh;
    overflow-y: auto;
  }

  .options {
    padding: 20px;
    display: flex;
    flex-direction: column;
    gap: 18px;
    overflow-y: auto;
  }

  .presets {
    display: grid;
    gap: 6px;
    margin-top: -8px;
  }

  .preset-row {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
  }

  .preset-row select,
  .preset-row input {
    flex: 1 1 140px;
    min-width: 0;
  }

  fieldset {
    border: none;
    padding: 0;
    margin: 0;
    display: grid;
    gap: 8px;
  }

  legend {
    font-weight: 600;
    margin-bottom: 8px;
    padding: 0;
    display: flex;
    align-items: baseline;
    gap: 8px;
  }

  .field {
    display: grid;
    gap: 5px;
  }

  .regex {
    margin-top: 8px;
  }

  .regex input {
    font-family: ui-monospace, "SF Mono", Menlo, Consolas, monospace;
  }

  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .chip {
    display: inline-flex;
    align-items: center;
    padding: 3px 10px;
    border: 1px solid var(--border);
    border-radius: 999px;
    font-size: 13px;
    cursor: pointer;
    user-select: none;
  }

  .chip input {
    position: absolute;
    opacity: 0;
    pointer-events: none;
  }

  .chip:has(input:focus-visible) {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }

  .chip.on {
    background: var(--accent-soft);
    border-color: var(--accent);
  }

  .chip.keep.on {
    background: var(--ok-soft);
    border-color: var(--ok);
  }

  .link {
    background: none;
    border: none;
    padding: 0;
    font: inherit;
    font-weight: 400;
    color: var(--accent);
    cursor: pointer;
  }

  .choice {
    display: flex;
    align-items: center;
    gap: 8px;
    cursor: pointer;
  }

  .indent {
    padding-left: 24px;
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }

  .indent.disabled {
    opacity: 0.55;
  }

  .inline input {
    width: 80px;
  }

  .dates input {
    flex: 1;
    min-width: 130px;
  }

  details summary {
    cursor: pointer;
    font-weight: 600;
  }

  .speed {
    display: grid;
    gap: 8px;
    margin-top: 10px;
  }

  .speed label {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 8px;
  }

  .unit {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--muted);
  }

  .unit input {
    width: 90px;
  }

  .footer {
    margin-top: auto;
    display: grid;
    gap: 10px;
    padding-top: 14px;
    border-top: 1px solid var(--border);
  }

  .problem {
    color: var(--danger);
  }

  .folder {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }

  .folder code {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: ui-monospace, "SF Mono", Menlo, Consolas, monospace;
    font-size: 12px;
    background: var(--panel-2);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 3px 6px;
  }

  .summary {
    margin: 0;
    padding-left: 18px;
    display: grid;
    gap: 2px;
  }
</style>
