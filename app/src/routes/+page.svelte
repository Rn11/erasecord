<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { SvelteMap, SvelteSet } from "svelte/reactivity";
  import { isTauri } from "@tauri-apps/api/core";
  import { open } from "@tauri-apps/plugin-dialog";
  import type { UnlistenFn } from "@tauri-apps/api/event";
  import { api, asCommandError, onJobEvent, onPreviewEntry } from "$lib/api";
  import { errorMessage } from "$lib/errors";
  import { applyDocumentLanguage, num, t } from "$lib/i18n.svelte";
  import LanguagePicker from "$lib/components/LanguagePicker.svelte";
  import {
    contentProblem,
    displayName,
    emptyContent,
    rangeProblem,
    toFilter,
    type ContentForm,
    type RangeForm,
  } from "$lib/format";
  import { forgetLastPackage, loadLastPackage, saveLastPackage, type LastPackage } from "$lib/lastPackage";
  import { loadLast, saveLast, snapshot } from "$lib/presets";
  import { newChoice, passphraseInput, type PassphraseChoice } from "$lib/passphrase";
  import OpenBackup from "$lib/components/OpenBackup.svelte";
  import { findUpdate, skipVersion } from "$lib/updates";
  import type { Update } from "@tauri-apps/plugin-updater";
  import { applyEvent, newRun, type RunState } from "$lib/run";
  import type {
    Filter,
    Friend,
    GuildChannel,
    JobOptions,
    PackageSummary,
    PassphraseInput,
    PreviewEntry,
    Target,
    UnfinishedRun,
    User,
  } from "$lib/types";
  import Avatar from "$lib/components/Avatar.svelte";
  import Login from "$lib/components/Login.svelte";
  import Preview from "$lib/components/Preview.svelte";
  import Progress from "$lib/components/Progress.svelte";
  import Setup from "$lib/components/Setup.svelte";
  import Insights from "$lib/components/insights/Insights.svelte";
  import { resetInsights } from "$lib/insights/store.svelte";
  import type { CleanUpRequest } from "$lib/insights/types";

  type Screen = "starting" | "login" | "setup" | "preview" | "progress";

  let screen = $state<Screen>("starting");
  /** The two parts of the app; Insights also works without logging in. */
  let tab = $state<"cleanup" | "insights">("cleanup");
  /** Using Insights without an account. */
  let offline = $state(false);
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
  /** The imported data package; while set, its servers and DMs replace the live ones. */
  let pkg = $state<PackageSummary | null>(null);
  let importing = $state(false);
  let importError = $state<string | null>(null);
  /** The package opened last time, to open again with one click. */
  let lastPackage = $state<LastPackage | null>(loadLastPackage());
  /** A file is being dragged over the window. */
  let dragging = $state(false);
  /** Friends without an open DM; null until asked for. */
  let friends = $state<Friend[] | null>(null);
  let friendsLoading = $state(false);
  let friendsError = $state<string | null>(null);
  const opening = new SvelteSet<string>();
  // The settings used last time.
  const last = loadLast();
  let range = $state<RangeForm>(last.range);
  let content = $state<ContentForm>(last.content);
  let skipPinned = $state(last.skipPinned);
  let options = $state<JobOptions>({ ...last.options, dry_run: false });

  // Fixed when counting starts, so the deletion uses exactly what was counted.
  let filter = $state<Filter>(toFilter({ mode: "all", amount: 1, unit: "days", from: "", to: "" }, true));
  let previewTargets = $state<Target[]>([]);
  let entries = $state<PreviewEntry[]>([]);
  let counting = $state(false);
  let previewError = $state<string | null>(null);

  let run = $state<RunState | null>(null);
  /** Encrypt backups (recommended); the passphrase lives in memory only. */
  let encrypt = $state(true);
  let passphrase = $state<PassphraseChoice | null>(null);
  /** The passphrase of the last run's backup, offered for saving its list. */
  let lastRunPassphrase = $state<PassphraseInput | null>(null);
  let resumePass = $state("");
  let resumePassFile = $state<string | null>(null);
  let resumeError = $state<string | null>(null);
  let openingBackup = $state(false);
  /** A newer version, if there is one. */
  let update = $state<Update | null>(null);
  let updating = $state<{ done: number; total: number | null } | null>(null);
  let updateError = $state<string | null>(null);
  /** A clean-up that was stopped or cut short and can be continued. */
  let unfinished = $state<UnfinishedRun | null>(null);

  const shownTargets = $derived(pkg ? pkg.targets.map((t) => t.target) : targets);
  const selectedTargets = $derived(
    shownTargets.filter((t) => selected.has(t.id)).map((t) => ({ ...t, channels: channelPicks.get(t.id) ?? [] })),
  );
  const busy = $derived(counting || (run !== null && run.summary === null));

  const unlisten: UnlistenFn[] = [];

  onMount(async () => {
    applyDocumentLanguage();
    if (import.meta.env.DEV && !isTauri()) {
      const { installMockBackend } = await import("$lib/mock");
      installMockBackend();
    }
    if (isTauri()) {
      // A data package dropped anywhere on the window is imported.
      const { getCurrentWebview } = await import("@tauri-apps/api/webview");
      unlisten.push(
        await getCurrentWebview().onDragDropEvent((event) => {
          const drag = event.payload;
          if (drag.type === "enter" || drag.type === "over") dragging = user !== null || offline;
          else if (drag.type === "leave") dragging = false;
          else if (drag.type === "drop") {
            dragging = false;
            const path = drag.paths[0];
            if (path && (user || offline)) importPackageFrom(path);
          }
        }),
      );
    }
    unlisten.push(
      await onJobEvent((event) => {
        if (run) applyEvent(run, event);
      }),
      await onPreviewEntry((entry) => {
        if (counting) entries.push(entry);
      }),
    );
    // Not urgent: look for a newer version once the app is up.
    setTimeout(async () => (update = await findUpdate()), 3000);
    try {
      const restored = await api.restoreSession();
      if (restored) await enter(restored);
      else screen = "login";
    } catch (err) {
      notice = t("page.savedTokenFailed", { error: errorMessage(err) });
      screen = "login";
    }
  });

  onDestroy(() => unlisten.forEach((stop) => stop()));

  /** Shows an error; sends the user back to the login if the token stopped working. */
  function fail(err: unknown): string | null {
    const error = asCommandError(err);
    if (error.kind === "unauthorized" || error.kind === "not_logged_in") {
      user = null;
      notice = t("page.tokenRejected");
      screen = "login";
      return null;
    }
    return errorMessage(error);
  }

  async function enter(loggedIn: User, rememberError: string | null = null) {
    user = loggedIn;
    offline = false;
    notice = rememberError ? t("page.rememberFailed", { error: rememberError }) : null;
    screen = "setup";
    await Promise.all([loadTargets(), checkUnfinished(), passphrase ? null : freshPassphrase()]);
  }

  async function checkUnfinished() {
    try {
      unfinished = await api.unfinishedRun();
    } catch {
      unfinished = null;
    }
  }

  async function continueRun() {
    if (!unfinished) return;
    const from = $state.snapshot(unfinished);
    const pass: PassphraseInput | null = from.encrypted_backup
      ? resumePassFile && !resumePass
        ? { file: resumePassFile }
        : { text: resumePass }
      : null;
    resumeError = null;
    unfinished = null;
    run = newRun(from.targets, 0, false, { finished: from.finished, stats: from.stats });
    screen = "progress";
    try {
      await api.resumeRun(pass);
      lastRunPassphrase = pass;
      resumePass = "";
    } catch (err) {
      const message = fail(err);
      if (message && from.encrypted_backup) {
        // Most likely a wrong passphrase: stay on the banner.
        run = null;
        screen = "setup";
        unfinished = from;
        resumeError = message;
      } else if (message && run) {
        run.summary = { stats: run.totals, cancelled: false, error: message };
        run.finishedAt = Date.now();
      }
    }
  }

  async function pickResumePassFile() {
    const file = await open({ multiple: false, directory: false, title: t("pass.pickFile") });
    if (typeof file === "string") resumePassFile = file;
  }

  async function installUpdate() {
    if (!update || busy) return;
    updateError = null;
    updating = { done: 0, total: null };
    try {
      await update.downloadAndInstall((event) => {
        if (!updating) return;
        if (event.event === "Started") updating.total = event.data.contentLength ?? null;
        else if (event.event === "Progress") updating.done += event.data.chunkLength;
      });
      const { relaunch } = await import("@tauri-apps/plugin-process");
      await relaunch();
    } catch (err) {
      updateError = errorMessage(err);
      updating = null;
    }
  }

  /** A fresh passphrase for the next encrypted backup. */
  async function freshPassphrase() {
    try {
      const next = newChoice(await api.generatePassphrase());
      // Someone using their own passphrase or a file keeps it.
      if (passphrase && passphrase.mode !== "generated") {
        Object.assign(next, { mode: passphrase.mode, own: passphrase.own, repeat: passphrase.repeat, file: passphrase.file });
      }
      passphrase = next;
    } catch {
      passphrase = null;
    }
  }

  async function discardRun() {
    await api.discardRun();
    unfinished = null;
  }

  async function loadTargets() {
    targetsLoading = true;
    targetsError = null;
    try {
      targets = await api.listTargets();
      friends = null;
      friendsError = null;
      // In package mode the lists show the package, not these targets.
      if (!pkg) {
        const known = new Set(targets.map((t) => t.id));
        for (const id of [...selected]) if (!known.has(id)) selected.delete(id);
        for (const id of [...channelPicks.keys()]) if (!known.has(id)) channelPicks.delete(id);
        channelLists.clear();
      }
    } catch (err) {
      targetsError = fail(err);
    } finally {
      targetsLoading = false;
    }
  }

  async function loadChannels(guildId: string) {
    // The package's channels were filled in on import.
    if (pkg) return;
    const current = channelLists.get(guildId);
    if (current && (current.loading || !current.error)) return;
    channelLists.set(guildId, { loading: true, error: null, channels: [] });
    try {
      const channels = await api.listChannels(guildId);
      channelLists.set(guildId, { loading: false, error: null, channels });
    } catch (err) {
      channelLists.set(guildId, { loading: false, error: fail(err) ?? t("page.tokenRejected"), channels: [] });
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
      if (message) friendsError = t("page.openDmFailed", { name: friend.name, error: message });
    } finally {
      opening.delete(friend.user_id);
    }
  }

  function clearSelection() {
    selected.clear();
    channelPicks.clear();
    channelLists.clear();
  }

  async function importPackage(folder: boolean) {
    importError = null;
    let path: string | null;
    try {
      path = await open({
        title: folder ? t("page.pickFolder") : t("page.pickZip"),
        directory: folder,
        multiple: false,
        filters: folder ? undefined : [{ name: t("page.packageFilter"), extensions: ["zip"] }],
      });
    } catch (err) {
      importError = errorMessage(err);
      return;
    }
    if (path) await importPackageFrom(path);
  }

  /** Reads the package at `path` (a .zip file or a folder). */
  async function importPackageFrom(path: string) {
    if (importing || busy) return;
    importError = null;
    importing = true;
    try {
      const summary = await api.importPackage(path);
      clearSelection();
      // The package knows each server's channels already.
      for (const item of summary.targets) {
        if (item.target.kind !== "guild") continue;
        const channels = item.channels.map((c) => ({ id: c.id, name: c.name, kind: 0, category: null, messages: c.messages }));
        channelLists.set(item.target.id, { loading: false, error: null, channels });
      }
      // The package cannot tell embeds and stickers apart.
      const known = (h: string) => h !== "embed" && h !== "sticker";
      content.has = content.has.filter(known);
      content.without = content.without.filter(known);
      resetInsights();
      pkg = summary;
      saveLastPackage(path, summary.messages);
      lastPackage = loadLastPackage();
    } catch (err) {
      importError = fail(err);
    } finally {
      importing = false;
    }
  }

  async function closePackage() {
    await api.closePackage();
    pkg = null;
    resetInsights();
    clearSelection();
  }

  function openInsightsOffline() {
    offline = true;
    tab = "insights";
  }

  /** Opens Clean up with what Insights found, ready to be counted. */
  function cleanUpFromInsights(request: CleanUpRequest) {
    if (!user) {
      notice = t("insights.loginToCleanUp");
      showLogin();
      return;
    }
    selected.clear();
    channelPicks.clear();
    for (const id of request.places) selected.add(id);
    if (request.channels.length > 0 && request.places.length === 1) {
      channelPicks.set(request.places[0], [...request.channels]);
    }
    range =
      request.from || request.to
        ? { ...range, mode: "between", from: request.from ?? "", to: request.to ?? "" }
        : { ...range, mode: "all" };
    content = { ...emptyContent(), contains: request.contains };
    notice = t("insights.handedOver");
    tab = "cleanup";
    screen = "setup";
  }

  function showLogin() {
    tab = "cleanup";
    screen = "login";
  }

  async function logout() {
    await api.logout();
    user = null;
    targets = [];
    clearSelection();
    friends = null;
    pkg = null;
    resetInsights();
    run = null;
    notice = null;
    offline = false;
    tab = "cleanup";
    screen = "login";
  }

  async function count() {
    if (selectedTargets.length === 0 || rangeProblem(range) || contentProblem(content)) return;
    filter = toFilter(range, skipPinned, content);
    saveLast(snapshot($state.snapshot(range), $state.snapshot(content), skipPinned, $state.snapshot(options)));
    previewTargets = selectedTargets;
    entries = [];
    previewError = null;
    counting = true;
    screen = "preview";
    try {
      entries = pkg
        ? await api.previewPackage($state.snapshot(previewTargets), $state.snapshot(filter))
        : await api.preview($state.snapshot(previewTargets), $state.snapshot(filter), $state.snapshot(options));
    } catch (err) {
      if (asCommandError(err).kind === "cancelled") screen = "setup";
      else previewError = fail(err);
    } finally {
      counting = false;
    }
  }

  async function start(dryRun: boolean) {
    // Places without matches need no work; failed searches get another try.
    // A new real run replaces the unfinished one.
    if (!dryRun) unfinished = null;
    const runTargets = entries.filter((e) => e.count !== 0).map((e) => e.target);
    const expected = entries.reduce((sum, e) => sum + (e.count ?? 0), 0);
    run = newRun($state.snapshot(runTargets), expected, dryRun);
    screen = "progress";
    try {
      const jobOptions = { ...$state.snapshot(options), dry_run: dryRun };
      const backupPassphrase =
        options.backup_dir !== null && encrypt && passphrase ? passphraseInput($state.snapshot(passphrase)) : null;
      if (pkg) await api.startPackageJob($state.snapshot(runTargets), $state.snapshot(filter), jobOptions, backupPassphrase);
      else await api.startJob($state.snapshot(runTargets), $state.snapshot(filter), jobOptions, backupPassphrase);
      lastRunPassphrase = backupPassphrase;
      // Every new backup gets new words.
      if (backupPassphrase && passphrase?.mode === "generated") void freshPassphrase();
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
    await Promise.all([loadTargets(), checkUnfinished()]);
  }
</script>

<div class="app">
  {#if openingBackup}
    <OpenBackup onClose={() => (openingBackup = false)} />
  {/if}
  {#if dragging}
    <div class="drop" aria-hidden="true">
      <div class="drop-box">{t("page.dropPackage")}</div>
    </div>
  {/if}
  {#if user || offline}
    <header class="topbar">
      <span class="brand">EraseCord</span>
      <nav class="tabs" aria-label={t("nav.label")}>
        <button class:active={tab === "cleanup"} aria-current={tab === "cleanup" ? "page" : undefined} onclick={() => (tab = "cleanup")}>
          {t("nav.cleanup")}
        </button>
        <button class:active={tab === "insights"} aria-current={tab === "insights" ? "page" : undefined} onclick={() => (tab = "insights")}>
          {t("nav.insights")}
        </button>
      </nav>
      <span class="spacer"></span>
      <LanguagePicker />
      {#if user}
        <span class="user">
          <Avatar
            name={displayName(user)}
            url={user.avatar ? `https://cdn.discordapp.com/avatars/${user.id}/${user.avatar}.png?size=64` : null}
            size={26}
          />
          {displayName(user)}
        </span>
        <button class="btn ghost small" onclick={logout} disabled={busy}>{t("common.logOut")}</button>
      {:else}
        <button class="btn small" onclick={showLogin}>{t("login.submit")}</button>
      {/if}
    </header>
  {/if}

  <main class:padded={tab === "insights" || (screen !== "login" && screen !== "starting")}>
    {#if update && screen !== "starting"}
      <div class="callout info small notice update">
        <span>
          <strong>{t("update.available", { version: update.version })}</strong>
          {#if update.body}
            <details class="notes">
              <summary>{t("update.whatsNew")}</summary>
              <pre>{update.body}</pre>
            </details>
          {/if}
          {#if updating}
            <span class="muted">
              {t("update.downloading")}
              {#if updating.total}{Math.round((updating.done / updating.total) * 100)} %{/if}
            </span>
          {/if}
          {#if updateError}<span class="error-text">{updateError}</span>{/if}
        </span>
        <span class="resume-actions">
          <button class="link small" onclick={() => update && (skipVersion(update.version), (update = null))} disabled={!!updating}>
            {t("update.skip")}
          </button>
          <button class="btn small" onclick={() => (update = null)} disabled={!!updating}>{t("update.later")}</button>
          <button class="btn primary small" onclick={installUpdate} disabled={!!updating || busy} title={busy ? t("update.busy") : ""}>
            {t("update.install")}
          </button>
        </span>
      </div>
    {/if}
    {#if tab === "cleanup" && screen === "setup" && unfinished}
      <div class="callout warn small notice resume">
        <span>
          <strong>{t("resume.title")}</strong>
          {t("resume.detail", {
            messages: t("count.message", { count: unfinished.stats.deleted }),
            left: num(unfinished.targets.length - unfinished.finished.length),
            total: num(unfinished.targets.length),
          })}
          <span class="muted">{t("resume.hint")}</span>
        </span>
        <span class="resume-actions">
          {#if unfinished.encrypted_backup}
            <input
              type="password"
              class="resume-pass"
              autocomplete="off"
              placeholder={resumePassFile ? t("open.usingFile") : t("resume.passphrase")}
              aria-label={t("resume.passphrase")}
              bind:value={resumePass}
            />
            <button class="link small" onclick={pickResumePassFile} title={resumePassFile ?? ""}>{t("resume.passFile")}</button>
          {/if}
          <button class="btn small" onclick={discardRun}>{t("resume.discard")}</button>
          <button
            class="btn primary small"
            onclick={continueRun}
            disabled={unfinished.encrypted_backup && !resumePass && !resumePassFile}>{t("resume.continue")}</button
          >
        </span>
      </div>
      {#if resumeError}<p class="callout error small notice">{resumeError}</p>{/if}
    {/if}
    {#if notice && user}
      <div class="callout info small notice">
        <span>{notice}</span>
        <button class="btn ghost small" onclick={() => (notice = null)} aria-label={t("common.dismiss")}>✕</button>
      </div>
    {/if}

    {#if screen === "starting"}
      <p class="starting muted"><span class="spinner"></span> {t("common.starting")}</p>
    {:else if tab === "insights" && (user || offline)}
      <Insights
        {pkg}
        {importing}
        {importError}
        {lastPackage}
        onImport={importPackage}
        onReopen={() => lastPackage && importPackageFrom(lastPackage.path)}
        onForget={() => ((lastPackage = null), forgetLastPackage())}
        onCleanUp={cleanUpFromInsights}
      />
    {:else if screen === "login" || !user}
      <Login {notice} onLogin={enter} onInsights={offline ? undefined : openInsightsOffline} />
    {:else if screen === "setup"}
      <Setup
        targets={shownTargets}
        {pkg}
        {importing}
        {importError}
        onImport={importPackage}
        onClosePackage={closePackage}
        onStats={() => (tab = "insights")}
        {lastPackage}
        onReopen={() => lastPackage && importPackageFrom(lastPackage.path)}
        onGuide={() => (tab = "insights")}
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
        bind:encrypt
        bind:passphrase
        onOpenBackup={() => (openingBackup = true)}
        onReload={loadTargets}
        onCount={count}
      />
    {:else if screen === "preview"}
      <Preview
        targets={previewTargets}
        {entries}
        {counting}
        exact={pkg !== null}
        error={previewError}
        {filter}
        onBack={() => (screen = "setup")}
        onCancel={() => api.cancelJob()}
        onStart={start}
      />
    {:else if screen === "progress" && run}
      <Progress
        {run}
        backupPassphrase={lastRunPassphrase}
        onPause={pause}
        onResume={resume}
        onStop={() => api.cancelJob()}
        onDone={finish}
      />
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

  .tabs {
    display: flex;
    gap: 2px;
    margin-left: 12px;
  }

  .tabs button {
    border: none;
    background: none;
    font: inherit;
    font-weight: 550;
    color: var(--muted);
    padding: 6px 12px;
    border-radius: 8px;
    cursor: pointer;
  }

  .tabs button:hover {
    color: var(--text);
  }

  .tabs button.active {
    background: var(--accent-soft);
    color: var(--text);
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

  .notes pre {
    white-space: pre-wrap;
    font: inherit;
    margin: 6px 0 0;
    max-height: 200px;
    overflow-y: auto;
  }

  .error-text {
    color: var(--danger);
  }

  .resume-pass {
    width: 180px;
  }

  .link {
    background: none;
    border: none;
    padding: 0;
    font: inherit;
    color: var(--accent);
    cursor: pointer;
  }

  .resume-actions {
    display: flex;
    align-items: center;
    gap: 6px;
    flex: none;
  }

  .drop {
    position: fixed;
    inset: 0;
    z-index: 100;
    display: grid;
    place-items: center;
    background: color-mix(in oklab, var(--bg) 70%, transparent);
    pointer-events: none;
  }

  .drop-box {
    padding: 28px 40px;
    border: 2px dashed var(--accent);
    border-radius: var(--radius);
    background: var(--panel);
    font-size: 16px;
    font-weight: 600;
  }

  .starting {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
  }
</style>
