import { i18n, joinAnd, joinOr, num, t, type Key } from "./i18n.svelte";
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

export const HAS_KINDS: Has[] = ["link", "file", "image", "video", "sound", "embed", "sticker"];

export function hasLabel(has: Has): string {
  return t(`has.${has}` as Key);
}

/** Why the content conditions cannot be used, or null. */
export function contentProblem(content: ContentForm): string | null {
  if (content.pattern.trim()) {
    try {
      new RegExp(content.pattern.trim(), "i");
    } catch {
      return t("problem.regex");
    }
  }
  const both = content.has.filter((h) => content.without.includes(h));
  if (both.length > 0) return t("problem.hasBoth", { kind: hasLabel(both[0]) });
  return null;
}

/** Why the form cannot be used yet, or null. */
export function rangeProblem(range: RangeForm): string | null {
  if (range.mode === "older_than" && !(Number.isInteger(range.amount) && range.amount > 0)) {
    return t("problem.amount");
  }
  if (range.mode === "between") {
    if (!range.from && !range.to) return t("problem.pickDate");
    if (range.from && range.to && range.from > range.to) return t("problem.dateOrder");
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

/** The conditions of a filter, one short statement each. */
export function filterLines(filter: Filter): string[] {
  const after = filter.after ? formatDate(filter.after) : null;
  const before = filter.before ? formatDate(filter.before) : null;
  const lines = [
    after && before
      ? t("filter.between", { after, before })
      : after
        ? t("filter.after", { date: after })
        : before
          ? t("filter.before", { date: before })
          : t("filter.anyTime"),
  ];
  if (filter.content) lines.push(t("filter.containing", { words: filter.content }));
  if (filter.pattern) lines.push(t("filter.matching", { pattern: filter.pattern }));
  if (filter.has.length) lines.push(t("filter.with", { kinds: joinOr(filter.has.map(lowerLabel)) }));
  if (filter.without.length) lines.push(t("filter.except", { kinds: joinOr(filter.without.map(lowerLabel)) }));
  return lines;
}

function lowerLabel(has: Has): string {
  // German nouns keep their capital letter.
  return i18n.locale === "de" ? hasLabel(has) : hasLabel(has).toLocaleLowerCase(i18n.locale);
}

/** "2 servers and 1 DM". */
export function describePlaces(servers: number, dms: number): string {
  const parts = [];
  if (servers) parts.push(t("count.server", { count: servers }));
  if (dms) parts.push(t("count.dm", { count: dms }));
  return joinAnd(parts);
}

export function formatDate(iso: string): string {
  return new Date(iso).toLocaleString(i18n.locale, { dateStyle: "medium", timeStyle: "short" });
}

export function formatDuration(ms: number): string {
  const unit = (value: number, name: "second" | "minute" | "hour") =>
    new Intl.NumberFormat(i18n.locale, { style: "unit", unit: name, unitDisplay: "short" }).format(value);
  const minutes = Math.round(ms / 60_000);
  if (ms < 60_000) return unit(Math.max(1, Math.round(ms / 1000)), "second");
  if (minutes < 60) return unit(minutes, "minute");
  return `${unit(Math.floor(minutes / 60), "hour")} ${unit(minutes % 60, "minute")}`;
}

function seconds(ms: number): string {
  return new Intl.NumberFormat(i18n.locale, {
    style: "unit",
    unit: "second",
    unitDisplay: "short",
    minimumFractionDigits: 1,
    maximumFractionDigits: 1,
  }).format(ms / 1000);
}

export function describeNotice(notice: Notice): string {
  switch (notice.kind) {
    case "rate_limited":
      return t(notice.global ? "notice.rateLimitedGlobal" : "notice.rateLimited", { seconds: seconds(notice.wait_ms) });
    case "index_not_ready":
      return t("notice.index", { seconds: seconds(notice.wait_ms) });
    case "retrying":
      return t("notice.retrying", {
        reason: notice.reason,
        attempt: String(notice.attempt),
        seconds: seconds(notice.wait_ms),
      });
  }
}

export function describeSkip(reason: SkipReason): string {
  return t(`skip.${reason}` as Key);
}

/** A one-line excerpt of a message for the activity log. */
export function messagePreview(content: string, attachments: number): string {
  const text = content.split(/\s+/).filter(Boolean).join(" ");
  if (text) return text.length > 100 ? `${text.slice(0, 100)}…` : text;
  return attachments ? t("preview.attachments", { count: attachments }) : t("preview.noPreview");
}

export function displayName(user: User): string {
  return user.global_name || user.username;
}

/** A target's name, with the number of channels if only some are selected. */
export function targetLabel(target: Target): string {
  return target.channels.length
    ? `${target.name} · ${t("count.channel", { count: target.channels.length })}`
    : target.name;
}

export { num };
