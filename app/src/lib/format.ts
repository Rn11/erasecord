import type { Filter, Notice, SkipReason, User } from "./types";

export type RangeMode = "all" | "older_than" | "between";
export type AgeUnit = "days" | "weeks" | "months" | "years";

/** The time range as entered in the form. */
export interface RangeForm {
  mode: RangeMode;
  amount: number;
  unit: AgeUnit;
  /** YYYY-MM-DD, local time; empty for "no limit". */
  from: string;
  /** YYYY-MM-DD, inclusive; empty for "no limit". */
  to: string;
}

/** Why the form cannot be used yet, or null. */
export function rangeProblem(range: RangeForm): string | null {
  if (range.mode === "older_than" && !(Number.isInteger(range.amount) && range.amount > 0)) {
    return "Enter a whole number greater than 0.";
  }
  if (range.mode === "between") {
    if (!range.from && !range.to) return "Pick at least one date.";
    if (range.from && range.to && range.from > range.to) return "The start date is after the end date.";
  }
  return null;
}

export function toFilter(range: RangeForm, skipPinned: boolean, now = new Date()): Filter {
  switch (range.mode) {
    case "all":
      return { after: null, before: null, skip_pinned: skipPinned };
    case "older_than":
      return {
        after: null,
        before: subtract(now, range.amount, range.unit).toISOString(),
        skip_pinned: skipPinned,
      };
    case "between": {
      const after = range.from ? localMidnight(range.from) : null;
      // The end date is inclusive, so stop at the following midnight.
      const before = range.to ? localMidnight(range.to, 1) : null;
      return {
        after: after?.toISOString() ?? null,
        before: before?.toISOString() ?? null,
        skip_pinned: skipPinned,
      };
    }
  }
}

function localMidnight(date: string, plusDays = 0): Date {
  const [year, month, day] = date.split("-").map(Number);
  return new Date(year, month - 1, day + plusDays);
}

function subtract(now: Date, amount: number, unit: AgeUnit): Date {
  const date = new Date(now);
  switch (unit) {
    case "days":
      date.setDate(date.getDate() - amount);
      break;
    case "weeks":
      date.setDate(date.getDate() - 7 * amount);
      break;
    case "months":
      date.setMonth(date.getMonth() - amount);
      break;
    case "years":
      date.setFullYear(date.getFullYear() - amount);
      break;
  }
  return date;
}

export function describeFilter(filter: Filter): string {
  const after = filter.after ? formatDate(filter.after) : null;
  const before = filter.before ? formatDate(filter.before) : null;
  if (after && before) return `sent from ${after} until before ${before}`;
  if (after) return `sent on or after ${after}`;
  if (before) return `sent before ${before}`;
  return "from any time";
}

export function formatDate(iso: string): string {
  return new Date(iso).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
}

export function formatDuration(ms: number): string {
  const minutes = Math.round(ms / 60_000);
  if (ms < 60_000) return `${Math.max(1, Math.round(ms / 1000))} s`;
  if (minutes < 60) return `${minutes} min`;
  const hours = Math.floor(minutes / 60);
  return `${hours} h ${minutes % 60} min`;
}

export function describeNotice(notice: Notice): string {
  const seconds = (ms: number) => `${(ms / 1000).toFixed(1)} s`;
  switch (notice.kind) {
    case "rate_limited":
      return `Rate limited by Discord${notice.global ? " (global)" : ""}, waiting ${seconds(notice.wait_ms)}`;
    case "index_not_ready":
      return `Discord is still indexing messages, waiting ${seconds(notice.wait_ms)}`;
    case "retrying":
      return `${notice.reason}; retry ${notice.attempt} in ${seconds(notice.wait_ms)}`;
  }
}

export function describeSkip(reason: SkipReason): string {
  switch (reason) {
    case "pinned":
      return "pinned";
    case "system_message":
      return "system message";
    case "no_permission":
      return "no permission";
    case "archived_thread":
      return "archived thread";
  }
}

export function displayName(user: User): string {
  return user.global_name || user.username;
}

export function plural(count: number, word: string): string {
  return `${count.toLocaleString()} ${word}${count === 1 ? "" : "s"}`;
}
