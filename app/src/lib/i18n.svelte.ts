// Translations. The language follows the system unless the user picks one;
// the choice is remembered in this browser profile only.

import { de } from "./locales/de";
import { en } from "./locales/en";
import { es } from "./locales/es";
import { sv } from "./locales/sv";
import { uk } from "./locales/uk";

export type Key = keyof typeof en;
export type PluralEntry = { one?: string; few?: string; many?: string; other: string };
export type Dictionary = { [K in Key]: (typeof en)[K] extends string ? string : PluralEntry };

const dictionaries = { en, de, es, sv, uk } satisfies Record<string, Dictionary>;
export type Locale = keyof typeof dictionaries;
export type Choice = Locale | "system";

/** Each language in its own name. */
export const LANGUAGES: { code: Locale; name: string }[] = [
  { code: "en", name: "English" },
  { code: "de", name: "Deutsch" },
  { code: "es", name: "Español" },
  { code: "sv", name: "Svenska" },
  { code: "uk", name: "Українська" },
];

const STORAGE_KEY = "erasecord.language";

function isLocale(value: unknown): value is Locale {
  return typeof value === "string" && value in dictionaries;
}

/** The first system language EraseCord knows, else English. */
export function systemLocale(): Locale {
  const tags = typeof navigator === "undefined" ? [] : (navigator.languages ?? [navigator.language]);
  for (const tag of tags) {
    const base = tag?.toLowerCase().split(/[-_]/)[0];
    if (isLocale(base)) return base;
  }
  return "en";
}

function savedChoice(): Choice {
  try {
    const value = localStorage.getItem(STORAGE_KEY);
    return isLocale(value) ? value : "system";
  } catch {
    return "system";
  }
}

const state = $state<{ choice: Choice }>({ choice: savedChoice() });

export const i18n = {
  get choice(): Choice {
    return state.choice;
  },
  get locale(): Locale {
    return state.choice === "system" ? systemLocale() : state.choice;
  },
  set(choice: Choice) {
    state.choice = choice;
    try {
      if (choice === "system") localStorage.removeItem(STORAGE_KEY);
      else localStorage.setItem(STORAGE_KEY, choice);
    } catch {
      // Not remembered, but still used for this session.
    }
    applyDocumentLanguage();
  },
};

export function applyDocumentLanguage() {
  if (typeof document !== "undefined") document.documentElement.lang = i18n.locale;
}

const pluralRules = new Map<Locale, Intl.PluralRules>();

function pluralForm(locale: Locale, entry: PluralEntry, count: number): string {
  let rules = pluralRules.get(locale);
  if (!rules) pluralRules.set(locale, (rules = new Intl.PluralRules(locale)));
  const form = rules.select(count) as keyof PluralEntry;
  return entry[form] ?? entry.other;
}

/** The text for `key` in the current language, with `{name}` parameters filled in. */
export function t(key: Key, params: Record<string, string | number> = {}): string {
  const locale = i18n.locale;
  const entry: string | PluralEntry = (dictionaries[locale] as Dictionary)[key] ?? (en as Dictionary)[key];
  const text = typeof entry === "string" ? entry : pluralForm(locale, entry, Number(params.count ?? 0));
  return text.replace(/\{(\w+)\}/g, (match, name: string) => {
    if (!(name in params)) return match;
    const value = params[name];
    return typeof value === "number" ? num(value) : value;
  });
}

export function num(value: number): string {
  return value.toLocaleString(i18n.locale);
}

/** "a, b and c" in the current language. */
export function joinAnd(items: string[]): string {
  return new Intl.ListFormat(i18n.locale, { type: "conjunction" }).format(items);
}

/** "a, b or c" in the current language. */
export function joinOr(items: string[]): string {
  return new Intl.ListFormat(i18n.locale, { type: "disjunction" }).format(items);
}
