// Typed wrappers around the Rust commands in src-tauri/src/commands.rs.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  Info,
  LinksReport,
  Overview,
  PlacesReport,
  Scope,
  SearchResult,
  Timeline,
  WordsReport,
} from "./insights/types";
import type {
  CommandError,
  Filter,
  Friend,
  GuildChannel,
  JobEvent,
  JobOptions,
  LoginResult,
  PackageSummary,
  UnfinishedRun,
  PreviewEntry,
  Target,
  User,
} from "./types";

export const api = {
  login: (token: string, remember: boolean) => invoke<LoginResult>("login", { token, remember }),
  restoreSession: () => invoke<User | null>("restore_session"),
  logout: () => invoke<void>("logout"),
  listTargets: () => invoke<Target[]>("list_targets"),
  listChannels: (guildId: string) => invoke<GuildChannel[]>("list_channels", { guildId }),
  listFriends: () => invoke<Friend[]>("list_friends"),
  openDm: (userId: string) => invoke<Target>("open_dm", { userId }),
  preview: (targets: Target[], filter: Filter, options: JobOptions) =>
    invoke<PreviewEntry[]>("preview", { targets, filter, options }),
  startJob: (targets: Target[], filter: Filter, options: JobOptions) =>
    invoke<void>("start_job", { targets, filter, options }),
  importPackage: (path: string) => invoke<PackageSummary>("import_package", { path }),
  closePackage: () => invoke<void>("close_package"),
  insightsInfo: () => invoke<Info>("insights_info"),
  insightsOverview: (scope: Scope) => invoke<Overview>("insights_overview", { scope }),
  insightsTime: (scope: Scope) => invoke<Timeline>("insights_time", { scope }),
  insightsPlaces: (scope: Scope) => invoke<PlacesReport>("insights_places", { scope }),
  insightsWords: (scope: Scope) => invoke<WordsReport>("insights_words", { scope }),
  insightsLinks: (scope: Scope) => invoke<LinksReport>("insights_links", { scope }),
  insightsSearch: (scope: Scope, query: string, limit: number) =>
    invoke<SearchResult>("insights_search", { scope, query, limit }),
  previewPackage: (targets: Target[], filter: Filter) =>
    invoke<PreviewEntry[]>("preview_package", { targets, filter }),
  startPackageJob: (targets: Target[], filter: Filter, options: JobOptions) =>
    invoke<void>("start_package_job", { targets, filter, options }),
  exportRun: (path: string) => invoke<number>("export_run", { path }),
  unfinishedRun: () => invoke<UnfinishedRun | null>("unfinished_run"),
  resumeRun: () => invoke<UnfinishedRun>("resume_run"),
  discardRun: () => invoke<void>("discard_run"),
  pauseJob: () => invoke<void>("pause_job"),
  resumeJob: () => invoke<void>("resume_job"),
  cancelJob: () => invoke<void>("cancel_job"),
};

export function onJobEvent(handler: (event: JobEvent) => void): Promise<UnlistenFn> {
  return listen<JobEvent>("job-event", (event) => handler(event.payload));
}

export function onPreviewEntry(handler: (entry: PreviewEntry) => void): Promise<UnlistenFn> {
  return listen<PreviewEntry>("preview-entry", (event) => handler(event.payload));
}

export function asCommandError(err: unknown): CommandError {
  if (err && typeof err === "object" && "kind" in err && "message" in err) {
    return err as CommandError;
  }
  return { kind: "other", message: String(err) };
}
