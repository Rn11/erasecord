// A fake backend so the UI can be worked on in a plain browser with
// `npm run dev`. Only loaded in dev builds outside Tauri (see +page.svelte).
// Log in with any token; "bad" is rejected.

import { emit } from "@tauri-apps/api/event";
import { mockIPC } from "@tauri-apps/api/mocks";
import type { Filter, GuildChannel, JobEvent, PreviewEntry, Stats, Target, User } from "./types";

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

async function simulate(selected: Target[], filter: Filter, dryRun: boolean) {
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
      await sleep(dryRun ? 15 : 70);
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
        case "start_job":
          job.paused = false;
          job.cancelled = false;
          void simulate(args.targets, args.filter, args.options.dry_run);
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
