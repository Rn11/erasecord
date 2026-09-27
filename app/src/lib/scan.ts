// The state of a count: Discord's numbers first, then the messages read
// page by page, with statistics.

import type { Waiting } from "./status";
import type { Activity, JobOptions, PreviewEntry, ScanEvent, ScanStats, Target } from "./types";

export interface PlaceCount {
  /** Discord's count, or the exact number once everything was read. */
  total: number | null;
  error: string | null;
  read: number;
  matching: number;
  complete: boolean;
}

export interface ScanState {
  /** Events of other counts are ignored. */
  id: number;
  targets: Target[];
  places: Record<string, PlaceCount>;
  stats: ScanStats | null;
  activity: Activity | null;
  waiting: Waiting | null;
  /** Still counting or reading. */
  running: boolean;
  /** Stopped before everything was read. */
  stopped: boolean;
  error: string | null;
  /** From the data package: exact, nothing to read. */
  exact: boolean;
}

let lastId = 0;

export function newScan(targets: Target[]): ScanState {
  return {
    id: ++lastId,
    targets,
    places: {},
    stats: null,
    activity: null,
    waiting: null,
    running: true,
    stopped: false,
    error: null,
    exact: false,
  };
}

export function fromPackage(targets: Target[], entries: PreviewEntry[], stats: ScanStats): ScanState {
  const scan = newScan(targets);
  for (const e of entries) {
    const count = e.count ?? 0;
    scan.places[e.target.id] = { total: count, error: e.error, read: count, matching: count, complete: true };
  }
  scan.stats = stats;
  scan.running = false;
  scan.exact = true;
  return scan;
}

function place(scan: ScanState, id: string): PlaceCount {
  return (scan.places[id] ??= { total: null, error: null, read: 0, matching: 0, complete: false });
}

export function applyScanEvent(scan: ScanState, event: ScanEvent) {
  switch (event.type) {
    case "counted": {
      const p = place(scan, event.target_id);
      p.total = event.error ? p.total : event.total;
      p.error = event.error;
      break;
    }
    case "read": {
      const p = place(scan, event.target_id);
      p.read = event.read;
      p.matching = event.matching;
      p.complete = event.complete;
      if (event.complete) p.total = event.matching;
      break;
    }
    case "activity":
      scan.activity = event.activity;
      break;
    case "notice":
      scan.waiting = { notice: event.notice, at: Date.now() };
      break;
    case "stats":
      scan.stats = event.stats;
      break;
    case "finished":
      scan.running = false;
      scan.activity = null;
      scan.stopped = event.cancelled;
      scan.error = event.error?.message ?? null;
      break;
  }
}

/** The best number for a place: exact once read, Discord's count before. */
export function countOf(p: PlaceCount | undefined): number {
  if (!p) return 0;
  return p.complete ? p.matching : (p.total ?? 0);
}

export function totalOf(scan: ScanState): number {
  return scan.targets.reduce((sum, t) => sum + countOf(scan.places[t.id]), 0);
}

/** Every place is counted (read or not). */
export function allCounted(scan: ScanState): boolean {
  return scan.targets.every((t) => {
    const p = scan.places[t.id];
    return p && (p.total !== null || p.error !== null);
  });
}

export function allRead(scan: ScanState): boolean {
  return scan.targets.every((t) => scan.places[t.id]?.complete || scan.places[t.id]?.error);
}

/** Messages read so far, and how many there are to read. */
export function readProgress(scan: ScanState): { read: number; of: number } {
  let read = 0;
  let of = 0;
  for (const t of scan.targets) {
    const p = scan.places[t.id];
    if (!p || p.error) continue;
    read += p.complete ? p.read : Math.min(p.read, p.total ?? p.read);
    of += p.complete ? p.read : Math.max(p.total ?? 0, p.read);
  }
  return { read, of };
}

/**
 * Pauses as whole, non-negative milliseconds (an emptied field gives
 * null), within what the backend accepts.
 */
export function sanitizeOptions(options: JobOptions): JobOptions {
  const ms = (value: unknown, fallback: number) =>
    typeof value === "number" && Number.isFinite(value) ? Math.min(Math.max(0, Math.round(value)), 600_000) : fallback;
  return {
    ...options,
    delete_delay_ms: ms(options.delete_delay_ms, 2500),
    search_delay_ms: ms(options.search_delay_ms, 3000),
    max_rounds: Math.min(Math.max(1, Math.round(Number(options.max_rounds) || 3)), 10),
  };
}

/** Roughly how long deleting `count` messages takes with these pauses. */
export function deleteTimeMs(count: number, options: JobOptions, unreadPages = 0): number {
  const request = 350;
  const perMessage = options.delete_delay_ms + request;
  const breaks = Math.floor(count / 100) * 12 * options.delete_delay_ms;
  const searching = (unreadPages + 1) * (options.search_delay_ms + 600);
  return count * perMessage + breaks + searching;
}
