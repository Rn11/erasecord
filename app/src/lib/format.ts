import type { Filter, Has, Notice, SkipReason, Target, User } from "./types";

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

/** Text and content conditions as entered in the form. */
export interface ContentForm {
  contains: string;
  pattern: string;
  has: Has[];
  without: Has[];
}

export const emptyContent = (): ContentForm => ({ contains: "", pattern: "", has: [], without: [] });

export const HAS_KINDS: { value: Has; label: string }[] = [
  { value: "link", label: "Links" },
  { value: "file", label: "Attachments" },
  { value: "image", label: "Images" },
  { value: "video", label: "Videos" },
  { value: "sound", label: "Audio" },
  { value: "embed", label: "Embeds" },
  { value: "sticker", label: "Stickers" },
];

function hasLabel(has: Has): string {
  return HAS_KINDS.find((k) => k.value === has)?.label.toLowerCase() ?? has;
}

/** Why the content conditions cannot be used, or null. */
export function contentProblem(content: ContentForm): string | null {
  if (content.pattern.trim()) {
    try {
      new RegExp(content.pattern.trim(), "i");
    } catch {
      return "The regular expression is not valid.";
    }
  }
  const both = content.has.filter((h) => content.without.includes(h));
  if (both.length > 0) return `“${hasLabel(both[0])}” cannot be both required and kept.`;
  return null;
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

export function toFilter(
  range: RangeForm,
  skipPinned: boolean,
  content: ContentForm = emptyContent(),
  now = new Date(),
): Filter {
  const base = {
    skip_pinned: skipPinned,
    content: content.contains.trim() || null,
    pattern: content.pattern.trim() || null,
    has: [...content.has],
    without: [...content.without],
  };
  switch (range.mode) {
    case "all":
      return { ...base, after: null, before: null };
    case "older_than":
      return { ...base, after: null, before: subtract(now, range.amount, range.unit).toISOString() };
    case "between": {
      const after = range.from ? localMidnight(range.from) : null;
      // The end date is inclusive, so stop at the following midnight.
      const before = range.to ? localMidnight(range.to, 1) : null;
      return { ...base, after: after?.toISOString() ?? null, before: before?.toISOString() ?? null };
    }
  }
}

/** Whether some conditions are only checked while deleting, so counts can be too high. */
export function checksLocally(filter: Filter): boolean {
  return !!filter.pattern || filter.without.length > 0;
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
  const parts = [
    after && before
      ? `sent from ${after} until before ${before}`
      : after
        ? `sent on or after ${after}`
        : before
          ? `sent before ${before}`
          : "from any time",
  ];
  if (filter.content) parts.push(`containing “${filter.content}”`);
  if (filter.pattern) parts.push(`matching /${filter.pattern}/`);
  if (filter.has.length) parts.push(`with ${listOr(filter.has.map(hasLabel))}`);
  if (filter.without.length) parts.push(`except those with ${listOr(filter.without.map(hasLabel))}`);
  return parts.join(", ");
}

function listOr(items: string[]): string {
  return items.length < 2 ? items.join("") : `${items.slice(0, -1).join(", ")} or ${items[items.length - 1]}`;
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
    case "excluded":
      return "excluded by filter";
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

/** A target's name, with the number of channels if only some are selected. */
export function targetLabel(target: Target): string {
  return target.channels.length ? `${target.name} · ${plural(target.channels.length, "channel")}` : target.name;
}
