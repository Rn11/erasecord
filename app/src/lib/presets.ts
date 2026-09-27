// Settings that are remembered between sessions: the last ones used, and
// named presets that also remember which servers and DMs were selected.
// Stored in the app's local storage, which stays on this device.

import { emptyContent, HAS_KINDS, type ContentForm, type RangeForm } from "./format";
import type { Has, JobOptions } from "./types";

export interface Settings {
  range: RangeForm;
  content: ContentForm;
  skipPinned: boolean;
  /** Without `dry_run`, which is chosen per run. */
  options: Omit<JobOptions, "dry_run">;
}

export interface Preset {
  name: string;
  settings: Settings;
  /** Selected servers and DMs. */
  targets: string[];
  /** Per server: the ticked channels, if only some were. */
  channels: Record<string, string[]>;
}

const LAST_KEY = "erasecord.settings";
const PRESETS_KEY = "erasecord.presets";

export const defaultSettings = (): Settings => ({
  range: { mode: "older_than", amount: 30, unit: "days", from: "", to: "" },
  content: emptyContent(),
  skipPinned: true,
  options: { delete_delay_ms: 1200, search_delay_ms: 2000, max_rounds: 3, overwrite: null, backup_dir: null },
});

function read(key: string): unknown {
  try {
    const value = localStorage.getItem(key);
    return value === null ? null : JSON.parse(value);
  } catch {
    return null;
  }
}

function write(key: string, value: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // Not remembering is fine.
  }
}

const isObject = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);
const pick = <T>(v: unknown, ok: (v: unknown) => boolean, fallback: T): T => (ok(v) ? (v as T) : fallback);
const isString = (v: unknown) => typeof v === "string";
const isCount = (v: unknown) => typeof v === "number" && Number.isFinite(v) && v >= 0;
const kinds = (v: unknown): Has[] => (Array.isArray(v) ? v.filter((k): k is Has => HAS_KINDS.includes(k)) : []);
const ids = (v: unknown): string[] => (Array.isArray(v) ? v.filter((id): id is string => isString(id)) : []);

/** Reads stored settings, keeping defaults for anything missing or odd, so
 * older or damaged data never breaks the form. */
function parseSettings(value: unknown): Settings {
  const d = defaultSettings();
  if (!isObject(value)) return d;
  const r = isObject(value.range) ? value.range : {};
  const c = isObject(value.content) ? value.content : {};
  const o = isObject(value.options) ? value.options : {};
  return {
    range: {
      mode: pick(r.mode, (m) => m === "older_than" || m === "between" || m === "all", d.range.mode),
      amount: pick(r.amount, (a) => isCount(a) && (a as number) >= 1, d.range.amount),
      unit: pick(r.unit, (u) => ["days", "weeks", "months", "years"].includes(u as string), d.range.unit),
      from: pick(r.from, isString, ""),
      to: pick(r.to, isString, ""),
    },
    content: {
      contains: pick(c.contains, isString, ""),
      pattern: pick(c.pattern, isString, ""),
      has: kinds(c.has),
      without: kinds(c.without),
    },
    skipPinned: pick(value.skipPinned, (v) => typeof v === "boolean", d.skipPinned),
    options: {
      delete_delay_ms: pick(o.delete_delay_ms, isCount, d.options.delete_delay_ms),
      search_delay_ms: pick(o.search_delay_ms, isCount, d.options.search_delay_ms),
      max_rounds: pick(o.max_rounds, isCount, d.options.max_rounds),
      overwrite: pick(o.overwrite, (v) => v === null || isString(v), null),
      backup_dir: pick(o.backup_dir, (v) => v === null || isString(v), null),
    },
  };
}

export function snapshot(range: RangeForm, content: ContentForm, skipPinned: boolean, options: JobOptions): Settings {
  const { dry_run: _, ...rest } = options;
  return structuredClone({ range, content, skipPinned, options: rest });
}

export function loadLast(): Settings {
  return parseSettings(read(LAST_KEY));
}

export function saveLast(settings: Settings) {
  write(LAST_KEY, settings);
}

export function loadPresets(): Preset[] {
  const value = read(PRESETS_KEY);
  if (!Array.isArray(value)) return [];
  return value
    .filter((p) => isObject(p) && isString(p.name) && (p.name as string).trim())
    .map((p) => ({
      name: (p.name as string).trim(),
      settings: parseSettings(p.settings),
      targets: ids(p.targets),
      channels: Object.fromEntries(
        Object.entries(isObject(p.channels) ? p.channels : {}).map(([k, v]) => [k, ids(v)] as const),
      ),
    }));
}

export function savePresets(presets: Preset[]) {
  write(PRESETS_KEY, presets);
}

/** Adds a preset, replacing one with the same name (ignoring case). */
export function withPreset(presets: Preset[], preset: Preset): Preset[] {
  const key = preset.name.toLocaleLowerCase();
  const rest = presets.filter((p) => p.name.toLocaleLowerCase() !== key);
  return [...rest, preset].sort((a, b) => a.name.localeCompare(b.name));
}
