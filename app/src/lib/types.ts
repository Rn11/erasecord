// Mirrors the serde types of erasecord-core. IDs are strings because
// JavaScript numbers cannot hold 64-bit integers.

export type Snowflake = string;

export interface User {
  id: Snowflake;
  username: string;
  global_name: string | null;
  avatar: string | null;
}

export type TargetKind = "guild" | "dm" | "group_dm";

export interface Target {
  kind: TargetKind;
  id: Snowflake;
  name: string;
  icon_url: string | null;
  /** For servers: only these channels; empty means all. */
  channels: Snowflake[];
}

/** A friend whose DM is closed. */
export interface Friend {
  user_id: Snowflake;
  name: string;
  icon_url: string | null;
}

export interface GuildChannel {
  id: Snowflake;
  name: string;
  /** Discord channel type: 0 text, 2 voice, 5 announcement, 13 stage. */
  kind: number;
  category: string | null;
  /** Only for channels from a data package: how many of your messages it holds. */
  messages?: number;
}

export interface PackageChannelInfo {
  id: Snowflake;
  name: string;
  messages: number;
}

/** A server or DM in the imported data package. */
export interface PackageTarget {
  target: Target;
  messages: number;
  channels: PackageChannelInfo[];
  first_message: string | null;
  last_message: string | null;
}

export interface PackageSummary {
  targets: PackageTarget[];
  messages: number;
  /** Servers in the package that you are no longer a member of. */
  left_servers: Snowflake[];
}

export type Has = "link" | "file" | "image" | "video" | "sound" | "embed" | "sticker";

export interface Filter {
  /** RFC 3339; only messages sent at or after this time. */
  after: string | null;
  /** RFC 3339; only messages sent before this time. */
  before: string | null;
  skip_pinned: boolean;
  /** Words that must all appear in the text. */
  content: string | null;
  /** Case-insensitive regular expression (Rust syntax), checked locally. */
  pattern: string | null;
  /** Only messages with any of these. */
  has: Has[];
  /** Keep messages with any of these. */
  without: Has[];
}

export interface JobOptions {
  delete_delay_ms: number;
  search_delay_ms: number;
  max_rounds: number;
  dry_run: boolean;
  /** Replace the text (empty: random letters) and remove attachments before deleting. */
  overwrite: string | null;
}

export interface Stats {
  deleted: number;
  skipped: number;
  failed: number;
}

export interface Summary {
  stats: Stats;
  cancelled: boolean;
  error: string | null;
}

export interface PreviewEntry {
  target: Target;
  count: number | null;
  error: string | null;
}

export interface LoginResult {
  user: User;
  remember_error: string | null;
}

export type Notice =
  | { kind: "rate_limited"; wait_ms: number; global: boolean }
  | { kind: "index_not_ready"; wait_ms: number }
  | { kind: "retrying"; reason: string; attempt: number; wait_ms: number };

export type SkipReason = "pinned" | "excluded" | "system_message" | "no_permission" | "archived_thread";

export type JobEvent =
  | { type: "target_started"; index: number; target_id: Snowflake; name: string }
  | { type: "target_estimate"; target_id: Snowflake; total: number }
  | {
      type: "deleted";
      target_id: Snowflake;
      channel_id: Snowflake;
      message_id: Snowflake;
      sent_at: string;
      preview: string;
      content: string;
      attachments: string[];
      dry_run: boolean;
    }
  | { type: "skipped"; target_id: Snowflake; message_id: Snowflake; reason: SkipReason }
  | { type: "failed"; target_id: Snowflake; message_id: Snowflake; error: string }
  | { type: "target_failed"; target_id: Snowflake; error: string }
  | { type: "channel_unreachable"; target_id: Snowflake; channel_id: Snowflake; messages: number; error: string }
  | { type: "target_finished"; target_id: Snowflake; stats: Stats }
  | { type: "notice"; notice: Notice }
  | ({ type: "finished" } & Summary);

export interface CommandError {
  kind: "unauthorized" | "cancelled" | "busy" | "not_logged_in" | "no_package" | "other";
  message: string;
}
