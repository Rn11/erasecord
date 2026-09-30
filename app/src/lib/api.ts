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
  Opened,
  PassphraseInput,
  PackageSummary,
  UnfinishedRun,
  ExportSettings,
  PackagePreview,
  ScanEvent,
  Target,
  User,
} from "./types";

export const api = {
  login: (token: string, remember: boolean) => invoke<LoginResult>("login", { token, remember }),
  restoreSession: () => invoke<User | null>("restore_session"),
  logout: () => invoke<void>("logout"),
  listTargets: (refresh = false) => invoke<Target[]>("list_targets", { refresh }),
  listChannels: (guildId: string) => invoke<GuildChannel[]>("list_channels", { guildId }),
  listFriends: () => invoke<Friend[]>("list_friends"),
  openDm: (userId: string) => invoke<Target>("open_dm", { userId }),
  startScan: (targets: Target[], filter: Filter, options: JobOptions, scanId: number) =>
    invoke<void>("start_scan", { targets, filter, options, scanId }),
  stopScan: () => invoke<void>("stop_scan"),
  startJob: (
    targets: Target[],
    filter: Filter,
    options: JobOptions,
    backupPassphrase: PassphraseInput | null,
    exportTo: ExportSettings | null,
  ) => invoke<string | null>("start_job", { targets, filter, options, backupPassphrase, export: exportTo }),
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
    invoke<PackagePreview>("preview_package", { targets, filter }),
  startPackageJob: (
    targets: Target[],
    filter: Filter,
    options: JobOptions,
    backupPassphrase: PassphraseInput | null,
    exportTo: ExportSettings | null,
  ) => invoke<string | null>("start_package_job", { targets, filter, options, backupPassphrase, export: exportTo }),
  exportRun: (path: string, passphrase: PassphraseInput | null) => invoke<number>("export_run", { path, passphrase }),
  generatePassphrase: () => invoke<string>("generate_passphrase"),
  openBackup: (path: string, passphrase: PassphraseInput, into: string) =>
    invoke<Opened>("open_backup", { path, passphrase, into }),
  unfinishedRun: () => invoke<UnfinishedRun | null>("unfinished_run"),
  resumeRun: (passphrase: PassphraseInput | null) => invoke<UnfinishedRun>("resume_run", { passphrase }),
  discardRun: () => invoke<void>("discard_run"),
  pauseJob: () => invoke<void>("pause_job"),
  resumeJob: () => invoke<void>("resume_job"),
  cancelJob: () => invoke<void>("cancel_job"),
};

export function onJobEvent(handler: (event: JobEvent) => void): Promise<UnlistenFn> {
  return listen<JobEvent>("job-event", (event) => handler(event.payload));
}

export function onScanEvent(handler: (event: ScanEvent & { scan_id: number }) => void): Promise<UnlistenFn> {
  return listen<ScanEvent & { scan_id: number }>("scan-event", (event) => handler(event.payload));
}

export function asCommandError(err: unknown): CommandError {
  if (err && typeof err === "object" && "kind" in err && "message" in err) {
    return err as CommandError;
  }
  return { kind: "other", message: String(err) };
}
