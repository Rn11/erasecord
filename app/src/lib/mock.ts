// A fake backend so the UI can be worked on in a plain browser with
// `npm run dev`. Only loaded in dev builds outside Tauri (see +page.svelte).
// Log in with any token; "bad" is rejected.

import { emit } from "@tauri-apps/api/event";
import { mockIPC } from "@tauri-apps/api/mocks";
import type { Info } from "./insights/types";
import type {
  Filter,
  Friend,
  GuildChannel,
  JobEvent,
  PackageSummary,
  ScanEvent,
  ScanStats,
  Stats,
  Target,
  UnfinishedRun,
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

// Insights come from the real Rust code: tools/fake_discord.py
// --insights-package PATH runs the CLI for each query.
const DEV_INSIGHTS = "http://127.0.0.1:8765/dev/insights";

async function devInsights(body: unknown): Promise<unknown> {
  const response = await fetch(DEV_INSIGHTS, { method: "POST", body: JSON.stringify(body) });
  const data = await response.json();
  if (!response.ok) throw { kind: "other", message: data.message ?? "Insights are not available" };
  return data;
}

/** A package summary for the package the dev server analyses, if any. */
async function devPackage(): Promise<PackageSummary | null> {
  try {
    const info = (await devInsights({ section: "info" })) as Info;
    return {
      messages: info.messages,
      left_servers: [],
      targets: info.places.map((p) => ({
        target: { kind: p.kind, id: p.id, name: p.name, icon_url: null, channels: [] },
        messages: p.messages,
        channels: p.channels.map((c) => ({ id: c.id, name: c.name, messages: 0 })),
        first_message: info.first_day,
        last_message: info.last_day,
      })),
    };
  } catch {
    return null;
  }
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

let exportPath: string | null = null;

const WORDS = "party tonight meme game later lol thanks weekend pizza stream music photo movie homework coffee".split(" ");

/** Made-up statistics for about `n` messages. */
function fakeStats(n: number, complete: boolean): ScanStats {
  const months: [string, number][] = [];
  let left = n;
  for (let m = 0; m < 18 && left > 0; m++) {
    const d = new Date(2025, 2 + m, 1);
    const count = Math.min(left, Math.round((n / 12) * (0.4 + Math.abs(Math.sin(m * 1.7)))));
    months.push([`${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}`, count]);
    left -= count;
  }
  const total = months.reduce((s, [, c]) => s + c, 0);
  const week = Array.from({ length: 7 }, (_, d) =>
    Array.from({ length: 24 }, (_, h) => Math.round((total / 300) * Math.max(0, Math.sin(((h - 8) / 24) * Math.PI * 2) + (d >= 5 ? 0.6 : 0.2)))),
  );
  return {
    messages: total,
    words: total * 7,
    characters: total * 38,
    with_files: Math.round(total * 0.12),
    images: Math.round(total * 0.09),
    videos: Math.round(total * 0.02),
    audio: Math.round(total * 0.01),
    links: Math.round(total * 0.08),
    pinned_kept: complete ? 2 : 0,
    first: months.length ? `${months[0][0]}-03T18:22:00Z` : null,
    last: months.length ? `${months[months.length - 1][0]}-21T22:05:00Z` : null,
    months,
    week,
    top_words: WORDS.map((w, i) => [w, Math.round(total / (i + 3))] as [string, number]),
    top_emoji: [
      ["😂", Math.round(total / 9)],
      ["👍🏽", Math.round(total / 14)],
      ["🫠", Math.round(total / 20)],
      ["🩷", Math.round(total / 25)],
      ["🎉", Math.round(total / 30)],
    ],
    busiest_day: months.length ? [`${months[0][0]}-14`, Math.round(total / 40) + 3] : null,
    longest: 1834,
  };
}

/** Counts, then "reads" page by page, like the backend's scan. */
async function mockScan(targets: Target[], filter: Filter, scanId: number) {
  const send = (event: ScanEvent) => emit("scan-event", { ...event, scan_id: scanId });
  const totals = new Map<string, number>();
  for (const target of targets) {
    if (job.cancelled) return send({ type: "finished", cancelled: true, error: null });
    await send({ type: "activity", activity: { kind: "counting", target_id: target.id } });
    await sleep(350);
    if (target.id === "3005") {
      await send({ type: "counted", target_id: target.id, total: 0, error: "Discord answered 403: Missing Access" });
      continue;
    }
    const total = countFor(target, filter);
    totals.set(target.id, total);
    await send({ type: "counted", target_id: target.id, total, error: null });
    const read = Math.min(25, total);
    await send({ type: "read", target_id: target.id, read, matching: read, complete: read >= total });
  }
  let readAll = 0;
  const all = [...totals.values()].reduce((a, b) => a + b, 0);
  for (const [id, total] of totals) {
    let read = Math.min(25, total);
    readAll += read;
    let page = 1;
    while (read < total) {
      if (job.cancelled) return send({ type: "finished", cancelled: true, error: null });
      page++;
      await send({ type: "activity", activity: { kind: "reading", target_id: id, page } });
      if (page === 4) {
        await send({ type: "notice", notice: { kind: "rate_limited", wait_ms: 2500, global: false } });
        await sleep(2500);
      }
      await sleep(180);
      const step = Math.min(25, total - read);
      read += step;
      readAll += step;
      const done = read >= total;
      const matching = done ? Math.round(total * 0.96) : read;
      await send({ type: "read", target_id: id, read, matching, complete: done });
      if (done) await send({ type: "counted", target_id: id, total: matching, error: null });
      await send({ type: "stats", stats: fakeStats(readAll, readAll >= all) });
    }
  }
  await send({ type: "stats", stats: fakeStats(all, true) });
  await send({ type: "finished", cancelled: false, error: null });
}

/** A stopped mock run, to try "Continue" in the browser. */
let unfinished: (UnfinishedRun & { filter: Filter }) | null = null;

async function simulate(
  selected: Target[],
  filter: Filter,
  dryRun: boolean,
  overwrite: boolean,
  resume: UnfinishedRun | null = null,
  encrypted = false,
  others = false,
) {
  const send = (event: JobEvent) => emit("job-event", event);
  const total: Stats = { deleted: 0, skipped: 0, failed: 0 };
  const finished = [...(resume?.finished ?? [])];
  let cancelled = false;
  for (const [index, target] of selected.entries()) {
    if (finished.includes(target.id)) continue;
    await send({ type: "target_started", index, target_id: target.id, name: target.name });
    const count = countFor(target, filter);
    await send({ type: "target_estimate", target_id: target.id, total: count });
    await send({ type: "activity", activity: { kind: "using_found", target_id: target.id, messages: count } });
    await sleep(400);
    await send({ type: "activity", activity: { kind: "deleting", target_id: target.id } });
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
      if (!dryRun && i % 100 === 99) {
        await send({ type: "activity", activity: { kind: "break", ms: 3000 } });
        await sleep(3000);
        await send({ type: "activity", activity: { kind: "deleting", target_id: target.id } });
      }
      if (!dryRun && i % 50 === 49) {
        await send({ type: "notice", notice: { kind: "rate_limited", wait_ms: 1500, global: false } });
        await sleep(1500);
      }
    }
    if (others && !cancelled && target.kind !== "guild") {
      await send({ type: "activity", activity: { kind: "searching_others", target_id: target.id, page: 1 } });
      await sleep(400);
      for (let i = 0; i < 3; i++) {
        stats.saved_from_others = (stats.saved_from_others ?? 0) + 1;
        await send({
          type: "saved_from_others",
          target_id: target.id,
          channel_id: target.id,
          message_id: String(800_000 + i),
          sent_at: new Date(Date.now() - (i + 3) * 86_400_000).toISOString(),
          author: "Alex",
          content: "look at this",
          attachments: ["https://cdn.discordapp.com/attachments/1/2/photo.jpg"],
          saved: Array.from({ length: i + 1 }, (_, n) => `attachments/${target.id}/${800_000 + i}_${n + 1}_photo.jpg`),
        });
        await sleep(150);
      }
    }
    total.deleted += stats.deleted;
    total.skipped += stats.skipped;
    total.failed += stats.failed;
    await send({ type: "target_finished", target_id: target.id, stats, complete: !cancelled });
    if (!cancelled) finished.push(target.id);
    if (cancelled) break;
  }
  if (!dryRun) {
    const prior = resume?.stats ?? { deleted: 0, skipped: 0, failed: 0 };
    unfinished = cancelled
      ? {
          targets: selected,
          finished,
          filter,
          from_package: false,
          encrypted_backup: encrypted,
          stats: {
            deleted: prior.deleted + total.deleted,
            skipped: prior.skipped + total.skipped,
            failed: prior.failed + total.failed,
          },
        }
      : null;
  }
  if (encrypted) {
    await send(
      cancelled
        ? { type: "backup_kept", folder: "/home/demo/EraseCord backup/erasecord-backup-20260927.parts", reason: "the clean-up was stopped; continue it to finish the backup" }
        : { type: "backup_sealed", archive: "/home/demo/EraseCord backup/erasecord-backup-20260927-181500.tar.age", files: 23, messages: total.deleted + (resume?.stats.deleted ?? 0) },
    );
  }
  await send({ type: "finished", stats: total, cancelled, error: null });
}

const MOCK_WORDS = "ocean ribbon tiger lemon castle river maple orbit velvet canyon ember harbor quartz meadow falcon pixel".split(" ");

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
        case "start_scan":
          job.cancelled = false;
          void mockScan(args.targets as Target[], args.filter as Filter, args.scanId as number);
          return null;
        case "stop_scan":
          job.cancelled = true;
          await sleep(100);
          return null;
        case "plugin:dialog|save":
          return "/home/demo/Documents/erasecord-export.csv";
        case "export_run":
          await sleep(200);
          return 42;
        case "save_png":
          return null;
        case "plugin:dialog|open":
          return args.options?.directory ? "/home/demo/EraseCord backup" : "/home/demo/Downloads/package.zip";
        case "import_package":
          await sleep(800);
          return (await devPackage()) ?? fakePackage();
        case "close_package":
          return null;
        case "insights_info":
          return devInsights({ section: "info" });
        case "insights_overview":
        case "insights_time":
        case "insights_places":
        case "insights_words":
        case "insights_links":
          return devInsights({ section: cmd.slice("insights_".length), scope: args.scope });
        case "insights_search":
          return devInsights({ query: args.query, limit: args.limit, scope: args.scope });
        case "preview_package":
          return {
            stats: fakeStats(400, true),
            entries: (args.targets as Target[]).map((target) => {
            const item = fakePackage().targets.find((t) => t.target.id === target.id);
            const channels = item?.channels.filter((c) => !target.channels.length || target.channels.includes(c.id)) ?? [];
            let count = channels.reduce((sum, c) => sum + c.messages, 0);
            if (args.filter.after || args.filter.before) count = Math.round(count * 0.4);
            if (args.filter.content || args.filter.has.length) count = Math.round(count * 0.15);
            return { target, count, error: null };
            }),
          };
        case "unfinished_run":
          return unfinished;
        case "discard_run":
          unfinished = null;
          return null;
        case "resume_run": {
          if (!unfinished) throw { kind: "other", message: "nothing to continue" };
          const run = unfinished;
          if (run.encrypted_backup) {
            await sleep(600);
            if (!args.passphrase?.text && !args.passphrase?.file) throw { kind: "other", message: "the backup's passphrase is needed" };
            if (args.passphrase?.text === "wrong") throw { kind: "other", message: "wrong passphrase, or the file is damaged" };
          }
          job.paused = false;
          job.cancelled = false;
          void simulate(run.targets, run.filter, false, false, run, run.encrypted_backup);
          return run;
        }
        case "plugin:updater|check":
          // Add ?update to the address to try the update banner.
          return location.search.includes("update")
            ? { rid: 1, currentVersion: "0.3.0", version: "0.3.1", date: null, body: "- Faster charts\n- Fixes", rawJson: {} }
            : null;
        case "plugin:updater|download_and_install":
          await sleep(1500);
          return null;
        case "plugin:process|restart":
          location.reload();
          return null;
        case "plugin:opener|reveal_item_in_dir":
          return null;
        case "generate_passphrase":
          return Array.from({ length: 12 }, () => MOCK_WORDS[Math.floor(Math.random() * MOCK_WORDS.length)]).join(" ");
        case "open_backup":
          await sleep(900);
          if (args.passphrase?.text === "wrong") throw { kind: "other", message: "wrong passphrase, or the file is damaged" };
          return { folder: args.into, files: 23, messages: 118 };
        case "start_package_job":
        case "start_job":
          if (args.export) {
            const ext = { csv: "csv", json: "json", json_lines: "jsonl" }[args.export.format as string];
            const kind = args.options.dry_run ? "dry-run" : "deleted";
            exportPath = `${args.export.dir}/erasecord-${kind}-20260927-181500.${ext}${args.backupPassphrase ? ".age" : ""}`;
          } else exportPath = null;
          job.paused = false;
          job.cancelled = false;
          void simulate(
            args.targets,
            args.filter,
            args.options.dry_run,
            args.options.overwrite !== null,
            null,
            !!args.options.backup_dir && !!args.backupPassphrase,
            !!args.options.backup_dir && !!args.options.backup_others,
          );
          return exportPath;
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
