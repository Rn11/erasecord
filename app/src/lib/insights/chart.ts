// Small helpers shared by the Insights charts.

import { i18n } from "$lib/i18n.svelte";

export const SERIES_COLORS = [1, 2, 3, 4, 5, 6].map((n) => `var(--series-${n})`);
export const OTHER_COLOR = "var(--series-other)";

/** Clean axis ticks from 0 to at least `max`, about four of them. */
export function ticks(max: number, count = 4): number[] {
  const raw = Math.max(max, 1) / count;
  const power = 10 ** Math.floor(Math.log10(raw));
  const step = Math.max(1, [1, 2, 2.5, 5, 10].map((f) => f * power).find((s) => s >= raw) ?? raw);
  const top = Math.ceil(Math.max(max, 1) / step) * step;
  return Array.from({ length: Math.round(top / step) + 1 }, (_, i) => i * step);
}

/** A bar growing up from `base` to `top`, rounded at the top only. */
export function columnPath(x: number, width: number, top: number, base: number, round = true): string {
  const h = base - top;
  if (h <= 0 || width <= 0) return "";
  const r = round ? Math.min(4, width / 2, h) : 0;
  return `M${x},${base}V${top + r}q0,-${r} ${r},-${r}h${width - 2 * r}q${r},0 ${r},${r}V${base}Z`;
}

export function compact(value: number): string {
  return new Intl.NumberFormat(i18n.locale, { notation: "compact", maximumFractionDigits: 1 }).format(value);
}

export function percent(value: number): string {
  return new Intl.NumberFormat(i18n.locale, { style: "percent", maximumFractionDigits: 1 }).format(value);
}

/** A local date from `YYYY-MM-DD` (or `YYYY-MM`). */
export function localDate(key: string): Date {
  const [y, m, d] = key.split("-").map(Number);
  return new Date(y, (m ?? 1) - 1, d ?? 1);
}

export function formatDate(value: Date | string, options: Intl.DateTimeFormatOptions = { dateStyle: "medium" }) {
  const date = typeof value === "string" ? (value.length <= 10 ? localDate(value) : new Date(value)) : value;
  return new Intl.DateTimeFormat(i18n.locale, options).format(date);
}

export function monthName(key: string, style: "long" | "short" = "long"): string {
  return formatDate(localDate(key), { month: style, year: "numeric" });
}

/** `YYYY-MM-DD` of a local date. */
export function isoDay(date: Date): string {
  const m = String(date.getMonth() + 1).padStart(2, "0");
  const d = String(date.getDate()).padStart(2, "0");
  return `${date.getFullYear()}-${m}-${d}`;
}

/** The last day of the month `YYYY-MM`. */
export function monthEnd(key: string): string {
  const start = localDate(key);
  return isoDay(new Date(start.getFullYear(), start.getMonth() + 1, 0));
}

export function addDays(key: string, days: number): string {
  const date = localDate(key);
  date.setDate(date.getDate() + days);
  return isoDay(date);
}

/** Weekday names, Monday first. */
export function weekdays(style: "short" | "long" = "short"): string[] {
  return Array.from({ length: 7 }, (_, i) => formatDate(new Date(2024, 0, 1 + i), { weekday: style }));
}

export function hourLabel(hour: number): string {
  return formatDate(new Date(2024, 0, 1, hour), { hour: "numeric" });
}

/** Five levels for a heatmap, from quantiles of the non-zero values, so a
 * few very busy days do not wash out the rest. */
export function levels(values: number[]): (value: number) => number {
  const sorted = values.filter((v) => v > 0).sort((a, b) => a - b);
  if (sorted.length === 0) return () => 0;
  const cut = [0.2, 0.4, 0.6, 0.8].map((q) => sorted[Math.min(sorted.length - 1, Math.floor(q * sorted.length))]);
  return (value) => (value <= 0 ? 0 : 1 + cut.filter((c) => value > c).length);
}
