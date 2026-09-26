// Typed wrappers around the Rust commands in src-tauri/src/commands.rs.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  CommandError,
  Filter,
  GuildChannel,
  JobEvent,
  JobOptions,
  LoginResult,
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
  preview: (targets: Target[], filter: Filter, options: JobOptions) =>
    invoke<PreviewEntry[]>("preview", { targets, filter, options }),
  startJob: (targets: Target[], filter: Filter, options: JobOptions) =>
    invoke<void>("start_job", { targets, filter, options }),
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
