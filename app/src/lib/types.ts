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
  /** Folder to save attachments to before deleting; null for no backup. */
  backup_dir: string | null;
  /** Set by the backend for an encrypted backup; the app sends null. */
  backup_encryption?: null;
  /** With a backup: also save what the others sent, in DMs and group chats only. */
  backup_others?: boolean;
}

/** A passphrase as typed, or the file it is in. */
export interface PassphraseInput {
  text?: string;
  file?: string;
}

/** What opening a backup unpacked. */
export interface Opened {
  folder: string;
  files: number;
  messages: number;
}

export interface Stats {
  deleted: number;
  skipped: number;
  failed: number;
  /** Messages of others whose files were saved. */
  saved_from_others?: number;
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

/** Statistics about the messages found while counting. */
export interface ScanStats {
  messages: number;
  words: number;
  characters: number;
  with_files: number;
  images: number;
  videos: number;
  audio: number;
  links: number;
  pinned_kept: number;
  first: string | null;
  last: string | null;
  months: [string, number][];
  /** Weekday (Monday first) × hour, local time. */
  week: number[][];
  top_words: [string, number][];
  top_emoji: [string, number][];
  busiest_day: [string, number] | null;
  longest: number;
}

/** What a clean-up or count is busy with. */
export type Activity =
  | { kind: "counting"; target_id: Snowflake }
  | { kind: "reading"; target_id: Snowflake; page: number }
  | { kind: "searching"; target_id: Snowflake; page: number; again: boolean }
  | { kind: "using_found"; target_id: Snowflake; messages: number }
  | { kind: "deleting"; target_id: Snowflake }
  | { kind: "backing_up"; target_id: Snowflake }
  | { kind: "searching_others"; target_id: Snowflake; page: number }
  | { kind: "checking_channel"; target_id: Snowflake }
  | { kind: "break"; ms: number }
  | { kind: "sealing_backup" };

export type ScanEvent =
  | { type: "counted"; target_id: Snowflake; total: number; error: string | null }
  | { type: "read"; target_id: Snowflake; read: number; matching: number; complete: boolean }
  | { type: "activity"; activity: Activity }
  | { type: "notice"; notice: Notice }
  | { type: "stats"; stats: ScanStats }
  | { type: "finished"; cancelled: boolean; error: CommandError | null };

export interface PackagePreview {
  entries: PreviewEntry[];
  stats: ScanStats;
}

export type ExportFormat = "csv" | "json" | "json_lines";

/** Save the messages of a run into this folder while it goes. */
export interface ExportSettings {
  dir: string;
  format: ExportFormat;
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
      /** Saved attachment files, relative to the backup folder. */
      saved: string[];
      dry_run: boolean;
    }
  | { type: "skipped"; target_id: Snowflake; message_id: Snowflake; reason: SkipReason }
  | { type: "failed"; target_id: Snowflake; message_id: Snowflake; error: string }
  | {
      type: "saved_from_others";
      target_id: Snowflake;
      channel_id: Snowflake;
      message_id: Snowflake;
      sent_at: string;
      author: string;
      content: string;
      attachments: string[];
      saved: string[];
    }
  | { type: "not_saved_from_others"; target_id: Snowflake; message_id: Snowflake; error: string }
  | { type: "target_failed"; target_id: Snowflake; error: string }
  | { type: "channel_unreachable"; target_id: Snowflake; channel_id: Snowflake; messages: number; error: string }
  | { type: "target_finished"; target_id: Snowflake; stats: Stats; complete: boolean }
  | { type: "notice"; notice: Notice }
  | { type: "activity"; activity: Activity }
  | { type: "backup_sealed"; archive: string; files: number; messages: number }
  | { type: "backup_kept"; folder: string; reason: string }
  | ({ type: "finished" } & Summary);

export interface CommandError {
  kind: "unauthorized" | "cancelled" | "busy" | "not_logged_in" | "no_package" | "other";
  message: string;
}

/** A clean-up that was stopped or cut short and can be continued. */
export interface UnfinishedRun {
  targets: Target[];
  finished: Snowflake[];
  stats: Stats;
  from_package: boolean;
  /** Continuing needs the backup's passphrase. */
  encrypted_backup: boolean;
}

