<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { SvelteMap, SvelteSet } from "svelte/reactivity";
  import { isTauri } from "@tauri-apps/api/core";
  import type { UnlistenFn } from "@tauri-apps/api/event";
  import { api, asCommandError, onJobEvent, onPreviewEntry } from "$lib/api";
  import {
    contentProblem,
    displayName,
    emptyContent,
    rangeProblem,
    toFilter,
    type ContentForm,
    type RangeForm,
  } from "$lib/format";
  import { applyEvent, newRun, type RunState } from "$lib/run";
  import type { Filter, Friend, GuildChannel, JobOptions, PreviewEntry, Target, User } from "$lib/types";
  import Avatar from "$lib/components/Avatar.svelte";
  import Login from "$lib/components/Login.svelte";
  import Preview from "$lib/components/Preview.svelte";
  import Progress from "$lib/components/Progress.svelte";
  import Setup from "$lib/components/Setup.svelte";

  type Screen = "starting" | "login" | "setup" | "preview" | "progress";

  let screen = $state<Screen>("starting");
  let user = $state<User | null>(null);
  let notice = $state<string | null>(null);

  let targets = $state<Target[]>([]);
  let targetsLoading = $state(false);
  let targetsError = $state<string | null>(null);
  const selected = new SvelteSet<string>();
  /** Server ID → the channels picked in it; no entry means the whole server. */
  const channelPicks = new SvelteMap<string, string[]>();
  /** Server ID → its channels, once loaded. */
  const channelLists = new SvelteMap<string, { loading: boolean; error: string | null; channels: GuildChannel[] }>();
  /** Friends without an open DM; null until asked for. */
  let friends = $state<Friend[] | null>(null);
  let friendsLoading = $state(false);
  let friendsError = $state<string | null>(null);
  const opening = new SvelteSet<string>();
  let range = $state<RangeForm>({ mode: "older_than", amount: 30, unit: "days", from: "", to: "" });
  let content = $state<ContentForm>(emptyContent());
  let skipPinned = $state(true);
  let options = $state<JobOptions>({ delete_delay_ms: 1200, search_delay_ms: 2000, max_rounds: 3, dry_run: false, overwrite: null });

  // Fixed when counting starts, so the deletion uses exactly what was counted.
  let filter = $state<Filter>(toFilter({ mode: "all", amount: 1, unit: "days", from: "", to: "" }, true));
  let previewTargets = $state<Target[]>([]);
  let entries = $state<PreviewEntry[]>([]);
  let counting = $state(false);
  let previewError = $state<string | null>(null);

  let run = $state<RunState | null>(null);

  const selectedTargets = $derived(
    targets.filter((t) => selected.has(t.id)).map((t) => ({ ...t, channels: channelPicks.get(t.id) ?? [] })),
  );
  const busy = $derived(counting || (run !== null && run.summary === null));

  const unlisten: UnlistenFn[] = [];

  onMount(async () => {
    if (import.meta.env.DEV && !isTauri()) {
      const { installMockBackend } = await import("$lib/mock");
      installMockBackend();
    }
    unlisten.push(
      await onJobEvent((event) => {
        if (run) applyEvent(run, event);
      }),
      await onPreviewEntry((entry) => {
        if (counting) entries.push(entry);
      }),
    );
    try {
      const restored = await api.restoreSession();
      if (restored) await enter(restored);
      else screen = "login";
    } catch (err) {
      notice = `Could not log in with the saved token: ${asCommandError(err).message}`;
      screen = "login";
    }
  });

  onDestroy(() => unlisten.forEach((stop) => stop()));

  /** Shows an error; sends the user back to the login if the token stopped working. */
  function fail(err: unknown): string | null {
    const error = asCommandError(err);
    if (error.kind === "unauthorized" || error.kind === "not_logged_in") {
      user = null;
      notice = "Discord no longer accepts your token. Please log in again.";
      screen = "login";
      return null;
    }
    return error.message;
  }

  async function enter(loggedIn: User, rememberError: string | null = null) {
    user = loggedIn;
    notice = rememberError ? `Logged in, but the token could not be remembered: ${rememberError}` : null;
    screen = "setup";
    await loadTargets();
  }

  async function loadTargets() {
    targetsLoading = true;
    targetsError = null;
    try {
      targets = await api.listTargets();
      const known = new Set(targets.map((t) => t.id));
      for (const id of [...selected]) if (!known.has(id)) selected.delete(id);
      for (const id of [...channelPicks.keys()]) if (!known.has(id)) channelPicks.delete(id);
      channelLists.clear();
      friends = null;
      friendsError = null;
    } catch (err) {
      targetsError = fail(err);
    } finally {
      targetsLoading = false;
    }
  }

  async function loadChannels(guildId: string) {
    const current = channelLists.get(guildId);
    if (current && (current.loading || !current.error)) return;
    channelLists.set(guildId, { loading: true, error: null, channels: [] });
    try {
      const channels = await api.listChannels(guildId);
      channelLists.set(guildId, { loading: false, error: null, channels });
    } catch (err) {
      channelLists.set(guildId, { loading: false, error: fail(err) ?? "Not logged in", channels: [] });
    }
  }

  async function loadFriends() {
    friendsLoading = true;
    friendsError = null;
    try {
      friends = await api.listFriends();
    } catch (err) {
      friendsError = fail(err);
    } finally {
      friendsLoading = false;
    }
  }

  /** Opens a friend's DM and selects it; it then shows up like any open DM. */
  async function openFriend(friend: Friend) {
    if (opening.has(friend.user_id)) return;
    opening.add(friend.user_id);
    friendsError = null;
    try {
      const target = await api.openDm(friend.user_id);
      if (!targets.some((t) => t.id === target.id)) {
        const servers = targets.filter((t) => t.kind === "guild");
        targets = [...servers, target, ...targets.filter((t) => t.kind !== "guild")];
      }
      selected.add(target.id);
      friends = friends?.filter((f) => f.user_id !== friend.user_id) ?? null;
    } catch (err) {
      const message = fail(err);
      if (message) friendsError = `Could not open the DM with ${friend.name}: ${message}`;
    } finally {
      opening.delete(friend.user_id);
    }
  }

  async function logout() {
    await api.logout();
    user = null;
    targets = [];
    selected.clear();
    channelPicks.clear();
    channelLists.clear();
    friends = null;
    run = null;
    notice = null;
    screen = "login";
  }

  async function count() {
    if (selectedTargets.length === 0 || rangeProblem(range) || contentProblem(content)) return;
    filter = toFilter(range, skipPinned, content);
    previewTargets = selectedTargets;
    entries = [];
    previewError = null;
    counting = true;
    screen = "preview";
    try {
      entries = await api.preview(
        $state.snapshot(previewTargets),
        $state.snapshot(filter),
        $state.snapshot(options),
      );
    } catch (err) {
      if (asCommandError(err).kind === "cancelled") screen = "setup";
      else previewError = fail(err);
    } finally {
      counting = false;
    }
  }

  async function start(dryRun: boolean) {
    // Places without matches need no work; failed searches get another try.
    const runTargets = entries.filter((e) => e.count !== 0).map((e) => e.target);
    const expected = entries.reduce((sum, e) => sum + (e.count ?? 0), 0);
    run = newRun($state.snapshot(runTargets), expected, dryRun);
    screen = "progress";
    try {
      await api.startJob($state.snapshot(runTargets), $state.snapshot(filter), {
        ...$state.snapshot(options),
        dry_run: dryRun,
      });
    } catch (err) {
      const message = fail(err);
      if (message && run) {
        run.summary = { stats: run.totals, cancelled: false, error: message };
        run.finishedAt = Date.now();
      }
    }
  }

  async function pause() {
    if (!run || run.pausedAt !== null) return;
    await api.pauseJob();
    run.pausedAt = Date.now();
  }

  async function resume() {
    if (!run || run.pausedAt === null) return;
    await api.resumeJob();
    run.pausedMs += Date.now() - run.pausedAt;
    run.pausedAt = null;
  }

  async function finish() {
    run = null;
    screen = "setup";
    await loadTargets();
  }
</script>

<div class="app">
  {#if user}
    <header class="topbar">
      <span class="brand">EraseCord</span>
      <span class="spacer"></span>
      <span class="user">
        <Avatar
          name={displayName(user)}
          url={user.avatar ? `https://cdn.discordapp.com/avatars/${user.id}/${user.avatar}.png?size=64` : null}
          size={26}
        />
        {displayName(user)}
      </span>
      <button class="btn ghost small" onclick={logout} disabled={busy}>Log out</button>
    </header>
  {/if}

  <main class:padded={screen !== "login" && screen !== "starting"}>
    {#if notice && user}
      <div class="callout info small notice">
        <span>{notice}</span>
        <button class="btn ghost small" onclick={() => (notice = null)} aria-label="Dismiss">✕</button>
      </div>
    {/if}

    {#if screen === "starting"}
      <p class="starting muted"><span class="spinner"></span> Starting…</p>
    {:else if screen === "login"}
      <Login {notice} onLogin={enter} />
    {:else if screen === "setup"}
      <Setup
        {targets}
        loading={targetsLoading}
        error={targetsError}
        {selected}
        {channelPicks}
        {channelLists}
        onLoadChannels={loadChannels}
        {friends}
        {friendsLoading}
        {friendsError}
        {opening}
        onLoadFriends={loadFriends}
        onOpenFriend={openFriend}
        bind:range
        bind:content
        bind:skipPinned
        bind:options
        onReload={loadTargets}
        onCount={count}
      />
    {:else if screen === "preview"}
      <Preview
        targets={previewTargets}
        {entries}
        {counting}
        error={previewError}
        {filter}
        onBack={() => (screen = "setup")}
        onCancel={() => api.cancelJob()}
        onStart={start}
      />
    {:else if screen === "progress" && run}
      <Progress {run} onPause={pause} onResume={resume} onStop={() => api.cancelJob()} onDone={finish} />
    {/if}
  </main>
</div>

<style>
  .app {
    height: 100%;
    display: flex;
    flex-direction: column;
  }

  .topbar {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 20px;
    background: var(--panel);
    border-bottom: 1px solid var(--border);
  }

  .brand {
    font-weight: 750;
    font-size: 16px;
    letter-spacing: -0.01em;
  }

  .spacer {
    flex: 1;
  }

  .user {
    display: flex;
    align-items: center;
    gap: 8px;
    font-weight: 550;
  }

  main {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 12px;
    overflow: auto;
  }

  main > :global(:last-child) {
    flex: 1;
    min-height: 0;
  }

  main.padded {
    padding: 16px 20px 20px;
  }

  .notice {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    flex: none !important;
  }

  .starting {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
  }
</style>
