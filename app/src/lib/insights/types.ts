// Mirrors crates/core/src/insights.rs.

import type { Snowflake } from "$lib/types";

export type PlaceKind = "guild" | "dm" | "group_dm";

export interface Scope {
  /** YYYY-MM-DD, inclusive, local time. */
  from: string | null;
  to: string | null;
  places: Snowflake[];
  channels: Snowflake[];
}

export interface Place {
  id: Snowflake;
  kind: PlaceKind;
  name: string;
  messages: number;
  channels: { id: Snowflake; name: string }[];
}

export interface Info {
  places: Place[];
  messages: number;
  first_day: string | null;
  last_day: string | null;
}

export interface MessageRef {
  id: Snowflake;
  sent_at: string;
  /** Index into Info.places. */
  place: number;
  channel: string;
  text: string;
  attachments: number;
}

export interface DayCount {
  date: string;
  messages: number;
}

export interface Span {
  from: string;
  to: string;
  days: number;
}

export interface Year {
  year: number;
  messages: number;
  words: number;
  active_days: number;
  top_place: number | null;
}

export interface Overview {
  messages: number;
  words: number;
  characters: number;
  attachments: number;
  with_attachments: number;
  without_text: number;
  links: number;
  places: number;
  active_days: number;
  span_days: number;
  first: MessageRef | null;
  last: MessageRef | null;
  busiest_day: DayCount | null;
  longest_streak: Span | null;
  longest_break: Span | null;
  years: Year[];
}

export interface Series {
  place: number;
  /** Colour slot, 0–5. */
  slot: number;
}

export interface Bucket {
  key: string;
  total: number;
  series: number[];
}

export interface Timeline {
  days: DayCount[];
  series: Series[];
  months: Bucket[];
  weeks: Bucket[];
  /** Weekday (Monday first) × hour. */
  week_hours: number[][];
}

export interface PlaceRow {
  place: number;
  messages: number;
  words: number;
  attachments: number;
  active_days: number;
  first: string;
  last: string;
  monthly: number[];
}

export interface ChannelRow {
  id: Snowflake;
  name: string;
  place: number;
  messages: number;
}

export interface PlacesReport {
  months: string[];
  places: PlaceRow[];
  channels: ChannelRow[];
}

export interface Count {
  key: string;
  count: number;
}

export interface Emoji {
  emoji: string;
  id: Snowflake | null;
  animated: boolean;
  count: number;
}

export interface Mention {
  id: Snowflake;
  name: string | null;
  count: number;
}

export interface WordsReport {
  words: Count[];
  total_words: number;
  distinct_words: number;
  emoji: Emoji[];
  total_emoji: number;
  mentions: Mention[];
  lengths: number[];
  monthly_length: [string, number][];
  longest: MessageRef | null;
}

/** Upper bounds of the length buckets, as in the backend. */
export const LENGTH_BUCKETS = [10, 25, 50, 100, 250, 500, 1000, 2000];

export interface LinksReport {
  links: number;
  messages_with_links: number;
  domains: Count[];
  attachments: number;
  kinds: Count[];
  extensions: Count[];
  months: string[];
  monthly: number[][];
}

export const FILE_KINDS = ["image", "video", "audio", "document", "archive", "other"] as const;

export interface SearchResult {
  total: number;
  places: [number, number][];
  messages: MessageRef[];
}

export type Section = "overview" | "time" | "places" | "words" | "links" | "search";

export const emptyScope = (): Scope => ({ from: null, to: null, places: [], channels: [] });
