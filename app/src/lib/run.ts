// State of a clean-up run as the UI shows it, built from the job events.

import { describeNotice, describeSkip, formatDate } from "./format";
import type { JobEvent, Stats, Summary, Target } from "./types";

export type TargetStatus = "pending" | "running" | "done" | "stopped" | "failed";

export interface TargetProgress {
  target: Target;
  status: TargetStatus;
  estimate: number | null;
  stats: Stats;
  error: string | null;
}

export type LogTone = "ok" | "muted" | "warn" | "error";

export interface LogLine {
  id: number;
  at: number;
  tone: LogTone;
  text: string;
}

export interface RunState {
  dryRun: boolean;
  targets: TargetProgress[];
  /** Matching messages according to the preview. */
  expected: number;
  totals: Stats;
  /** The target being worked on. */
  currentId: string | null;
  startedAt: number;
  pausedAt: number | null;
  pausedMs: number;
  finishedAt: number | null;
  summary: Summary | null;
  log: LogLine[];
}

const MAX_LOG_LINES = 500;
let nextLogId = 0;

const noStats = (): Stats => ({ deleted: 0, skipped: 0, failed: 0 });

export function newRun(targets: Target[], expected: number, dryRun: boolean): RunState {
  return {
    dryRun,
    targets: targets.map((target) => ({ target, status: "pending", estimate: null, stats: noStats(), error: null })),
    expected,
    totals: noStats(),
    currentId: null,
    startedAt: Date.now(),
    pausedAt: null,
    pausedMs: 0,
    finishedAt: null,
    summary: null,
    log: [],
  };
}

export function processed(stats: Stats): number {
  return stats.deleted + stats.skipped + stats.failed;
}

/** Milliseconds spent working, without pauses. */
export function activeMs(run: RunState, now: number): number {
  const end = run.finishedAt ?? run.pausedAt ?? now;
  return Math.max(0, end - run.startedAt - run.pausedMs);
}

function log(run: RunState, tone: LogTone, text: string) {
  run.log.push({ id: nextLogId++, at: Date.now(), tone, text });
  if (run.log.length > MAX_LOG_LINES) run.log.splice(0, run.log.length - MAX_LOG_LINES);
}

export function applyEvent(run: RunState, event: JobEvent) {
  const progress =
    "target_id" in event ? run.targets.find((t) => t.target.id === event.target_id) : undefined;
  const name = progress?.target.name ?? "";

  switch (event.type) {
    case "target_started":
      if (progress) progress.status = "running";
      run.currentId = event.target_id;
      log(run, "muted", `Searching ${event.name}…`);
      break;
    case "target_estimate":
      if (progress) progress.estimate = event.total;
      break;
    case "deleted": {
      if (progress) progress.stats.deleted++;
      run.totals.deleted++;
      const verb = event.dry_run ? "Would delete" : "Deleted";
      log(run, "ok", `${verb} · ${name} · ${formatDate(event.sent_at)} · ${event.preview}`);
      break;
    }
    case "skipped":
      if (progress) progress.stats.skipped++;
      run.totals.skipped++;
      log(run, "muted", `Skipped (${describeSkip(event.reason)}) · ${name}`);
      break;
    case "failed":
      if (progress) progress.stats.failed++;
      run.totals.failed++;
      log(run, "error", `Failed · ${name} · ${event.error}`);
      break;
    case "target_failed":
      if (progress) {
        progress.status = "failed";
        progress.error = event.error;
      }
      log(run, "error", `Could not search ${name}: ${event.error}`);
      break;
    case "target_finished":
      if (progress) {
        progress.stats = event.stats;
        if (progress.status !== "failed") progress.status = "done";
      }
      break;
    case "notice":
      log(run, "warn", describeNotice(event.notice));
      break;
    case "finished":
      run.summary = { stats: event.stats, cancelled: event.cancelled, error: event.error };
      run.finishedAt = Date.now();
      run.totals = { ...event.stats };
      if (run.pausedAt !== null) {
        run.pausedMs += Date.now() - run.pausedAt;
        run.pausedAt = null;
      }
      // The target that was interrupted reported "finished" as well.
      if (event.cancelled || event.error) {
        const current = run.targets.find((t) => t.target.id === run.currentId);
        if (current && current.status !== "failed") current.status = "stopped";
      }
      for (const t of run.targets) if (t.status === "running") t.status = "done";
      log(run, event.error ? "error" : "muted", event.error ? `Stopped: ${event.error}` : event.cancelled ? "Stopped." : "Finished.");
      break;
  }
}
