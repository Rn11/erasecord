// The status line: what a count or clean-up is doing right now, in words.

import { num, t } from "./i18n.svelte";
import type { Activity, Notice } from "./types";

/** A notice that makes EraseCord wait, and when it arrived. */
export interface Waiting {
  notice: Notice;
  at: number;
}

export function activityText(activity: Activity, placeName: (id: string) => string): string {
  switch (activity.kind) {
    case "counting":
      return t("status.counting", { place: placeName(activity.target_id) });
    case "reading":
      return t("status.reading", { place: placeName(activity.target_id), page: num(activity.page) });
    case "searching":
      return activity.again
        ? t("status.checkingAgain", { place: placeName(activity.target_id) })
        : t("status.searching", { place: placeName(activity.target_id), page: num(activity.page) });
    case "using_found":
      return t("status.usingFound", { place: placeName(activity.target_id), count: num(activity.messages) });
    case "deleting":
      return t("status.deleting", { place: placeName(activity.target_id) });
    case "backing_up":
      return t("status.backingUp", { place: placeName(activity.target_id) });
    case "searching_others":
      return t("status.searchingOthers", { place: placeName(activity.target_id), page: num(activity.page) });
    case "checking_channel":
      return t("status.checkingChannel", { place: placeName(activity.target_id) });
    case "break":
      return t("status.break", { seconds: num(Math.ceil(activity.ms / 1000)) });
    case "sealing_backup":
      return t("status.sealing");
  }
}

/** Seconds left of a wait, or null once it is over. */
export function secondsLeft(waiting: Waiting | null, now: number): number | null {
  if (!waiting) return null;
  const left = waiting.at + waiting.notice.wait_ms - now;
  return left > 0 ? Math.ceil(left / 1000) : null;
}

export function waitingText(waiting: Waiting, seconds: number): string {
  const n = waiting.notice;
  switch (n.kind) {
    case "rate_limited":
      return t("status.rateLimited", { seconds: num(seconds) });
    case "index_not_ready":
      return t("status.indexing", { seconds: num(seconds) });
    case "retrying":
      return t("status.retrying", { seconds: num(seconds) });
  }
}
