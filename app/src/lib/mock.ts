// A fake backend so the UI can be worked on in a plain browser with
// `npm run dev`. Only loaded in dev builds outside Tauri (see +page.svelte).
// Log in with any token; "bad" is rejected.

import { emit } from "@tauri-apps/api/event";
import { mockIPC } from "@tauri-apps/api/mocks";
import type {
  Filter,
  Friend,
  GuildChannel,
  JobEvent,
  PackageSummary,
  PreviewEntry,
  Stats,
  Target,
  User,
} from "./types";

const me: User = { id: "1000", username: "demo", global_name: "Demo User", avatar: null };

const targets: Target[] = [
  ["guild", "3001", "Rust Enjoyers"],
  ["guild", "3002", "Gaming Night"],
  ["guild", "3003", "Photography Club"],
  ["guild", "3004", "Uni – Computer Science 2023"],
  ["guild", "3005", "Old Minecraft Server"],
  ["guild", "3006", "Home Lab"],
  ["guild", "3007", "Language Exchange"],
  ["dm", "4001", "Alice"],
  ["dm", "4002", "Bob"],
  ["group_dm", "4003", "Alice, Bob, Charlie"],
  ["dm", "4004", "Dana"],
].map(([kind, id, name]) => ({ kind, id, name, icon_url: null, channels: [] }) as Target);

const friends: Friend[] = [
  { user_id: "5001", name: "Erin", icon_url: null },
  { user_id: "5002", name: "Frank", icon_url: null },
  { user_id: "5003", name: "Grace", icon_url: null },
];

function fakePackage(): PackageSummary {
  const target = (kind: Target["kind"], id: string, name: string): Target => ({ kind, id, name, icon_url: null, channels: [] });
  const year = 365 * 86_400_000;
  const dates = { first_message: new Date(Date.now() - 4 * year).toISOString(), last_message: new Date().toISOString() };
  return {
    messages: 2451,
    left_servers: ["3099"],
    targets: [
      { target: target("guild", "3099", "Server I left"), messages: 310, channels: [{ id: "30991", name: "general", messages: 310 }], ...dates },
      {
        target: target("guild", "3001", "Rust Enjoyers"),
        messages: 1204,
        channels: [
          { id: "30012", name: "general", messages: 900 },
          { id: "30013", name: "memes", messages: 250 },
          { id: "30017", name: "old-channel", messages: 54 },
        ],
        ...dates,
      },
      { target: target("dm", "4001", "Alice"), messages: 512, channels: [{ id: "4001", name: "Alice", messages: 512 }], ...dates },
      { target: target("dm", "4009", "Old friend"), messages: 425, channels: [{ id: "4009", name: "Old friend", messages: 425 }], ...dates },
    ],
  };
}

function channelsOf(guildId: string): GuildChannel[] {
  const make = (n: number, name: string, category: string | null, kind = 0): GuildChannel => ({
    id: `${guildId}${n}`,
    name,
    kind,
    category,
  });
  return [
    make(1, "rules", null),
    make(2, "general", "Text channels"),
    make(3, "memes", "Text channels"),
    make(4, "off-topic", "Text channels"),
    make(5, "announcements", "Info", 5),
    make(6, "Lounge", "Voice channels", 2),
  ];
}

const samples = [
  "haha yes",
  "see you tomorrow",
  "did anyone try the new build?",
  "check this out https://example.com",
  "(1 attachment)",
  "ok",
  "that was a great game last night, same time next week?",
];

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

function countFor(target: Target, filter: Filter): number {
  if (target.id === "3007") return 0;
  let count = ((Number(target.id) * 37) % 160) + 8;
  if (filter.after || filter.before) count = Math.round(count * 0.4);
  if (filter.content || filter.has.length) count = Math.round(count * 0.15);
  if (target.channels.length) count = Math.round((count * target.channels.length) / 6);
  return count;
}

const job = { paused: false, cancelled: false };

/** Waits while paused; false once cancelled. */
async function checkpoint(): Promise<boolean> {
  while (job.paused && !job.cancelled) await sleep(100);
  return !job.cancelled;
}

async function simulate(selected: Target[], filter: Filter, dryRun: boolean, overwrite: boolean) {
  const send = (event: JobEvent) => emit("job-event", event);
  const total: Stats = { deleted: 0, skipped: 0, failed: 0 };
  let cancelled = false;
  for (const [index, target] of selected.entries()) {
    await send({ type: "target_started", index, target_id: target.id, name: target.name });
    const count = countFor(target, filter);
    await send({ type: "target_estimate", target_id: target.id, total: count });
    const stats: Stats = { deleted: 0, skipped: 0, failed: 0 };
    for (let i = 0; i < count; i++) {
      if (!(await checkpoint())) {
        cancelled = true;
        break;
      }
      await sleep(dryRun ? 15 : overwrite ? 140 : 70);
      const message_id = String(900_000 + i);
      if ((filter.pattern || filter.without.length) && i % 5 === 2) {
        stats.skipped++;
        await send({ type: "skipped", target_id: target.id, message_id, reason: "excluded" });
      } else if (filter.skip_pinned && i % 23 === 7) {
        stats.skipped++;
        await send({ type: "skipped", target_id: target.id, message_id, reason: "pinned" });
      } else if (i % 41 === 11) {
        stats.skipped++;
        await send({ type: "skipped", target_id: target.id, message_id, reason: "no_permission" });
      } else if (i % 97 === 50) {
        stats.failed++;
        await send({ type: "failed", target_id: target.id, message_id, error: "Discord answered 500: Internal Server Error" });
      } else {
        stats.deleted++;
        await send({
          type: "deleted",
          target_id: target.id,
          channel_id: target.id,
          message_id,
          sent_at: new Date(Date.now() - (i + 40) * 86_400_000).toISOString(),
          preview: samples[i % samples.length],
          content: samples[i % samples.length],
          attachments: [],
          saved: [],
          dry_run: dryRun,
        });
      }
      if (!dryRun && i % 50 === 49) {
        await send({ type: "notice", notice: { kind: "rate_limited", wait_ms: 1500, global: false } });
        await sleep(1500);
      }
    }
    total.deleted += stats.deleted;
    total.skipped += stats.skipped;
    total.failed += stats.failed;
    await send({ type: "target_finished", target_id: target.id, stats });
    if (cancelled) break;
  }
  await send({ type: "finished", stats: total, cancelled, error: null });
}

export function installMockBackend() {
  mockIPC(
    async (cmd, payload) => {
      const args = (payload ?? {}) as Record<string, any>;
      switch (cmd) {
        case "restore_session":
          return null;
        case "login":
          await sleep(400);
          if (args.token === "bad") {
            throw { kind: "unauthorized", message: "Discord rejected the token (401): it is wrong or has expired" };
          }
          return { user: me, remember_error: null };
        case "logout":
          return null;
        case "list_targets":
          await sleep(300);
          return targets;
        case "list_friends":
          await sleep(400);
          return friends.filter((f) => !targets.some((t) => t.id === `6${f.user_id}`));
        case "open_dm": {
          await sleep(300);
          const friend = friends.find((f) => f.user_id === args.userId);
          if (!friend) throw { kind: "other", message: "Discord answered 400: Unknown User" };
          const target: Target = { kind: "dm", id: `6${friend.user_id}`, name: friend.name, icon_url: null, channels: [] };
          if (!targets.some((t) => t.id === target.id)) targets.push(target);
          return target;
        }
        case "list_channels":
          await sleep(300);
          if (args.guildId === "3005") throw { kind: "other", message: "Discord answered 403: Missing Access" };
          return channelsOf(args.guildId);
        case "preview": {
          job.cancelled = false;
          const entries: PreviewEntry[] = [];
          for (const target of args.targets as Target[]) {
            await sleep(250);
            if (job.cancelled) throw { kind: "cancelled", message: "cancelled" };
            const entry: PreviewEntry =
              target.id === "3005"
                ? { target, count: null, error: "Discord answered 403: Missing Access" }
                : { target, count: countFor(target, args.filter), error: null };
            entries.push(entry);
            await emit("preview-entry", entry);
          }
          return entries;
        }
        case "plugin:dialog|save":
          return "/home/demo/Documents/erasecord-export.csv";
        case "export_run":
          await sleep(200);
          return 42;
        case "plugin:dialog|open":
          return args.options?.directory ? "/home/demo/EraseCord backup" : "/home/demo/Downloads/package.zip";
        case "import_package":
          await sleep(800);
          return fakePackage();
        case "close_package":
          return null;
        case "preview_package":
          return (args.targets as Target[]).map((target) => {
            const item = fakePackage().targets.find((t) => t.target.id === target.id);
            const channels = item?.channels.filter((c) => !target.channels.length || target.channels.includes(c.id)) ?? [];
            let count = channels.reduce((sum, c) => sum + c.messages, 0);
            if (args.filter.after || args.filter.before) count = Math.round(count * 0.4);
            if (args.filter.content || args.filter.has.length) count = Math.round(count * 0.15);
            return { target, count, error: null };
          });
        case "start_package_job":
        case "start_job":
          job.paused = false;
          job.cancelled = false;
          void simulate(args.targets, args.filter, args.options.dry_run, args.options.overwrite !== null);
          return null;
        case "pause_job":
          job.paused = true;
          return null;
        case "resume_job":
          job.paused = false;
          return null;
        case "cancel_job":
          job.cancelled = true;
          return null;
        case "plugin:opener|open_url":
          window.open(args.url, "_blank", "noopener");
          return null;
      }
      throw { kind: "other", message: `mock backend does not know ${cmd}` };
    },
    { shouldMockEvents: true },
  );
}
