// What the Insights screen shows, kept while switching tabs, and answers
// cached per question: going back to a section or a range is instant.

import { api } from "$lib/api";
import type { PackageSummary } from "$lib/types";
import { emptyScope, type Info, type Scope, type Section } from "./types";

export const insights = $state({
  section: "overview" as Section,
  scope: emptyScope(),
  info: null as Info | null,
  infoError: null as string | null,
  /** Bumped for every newly imported package, which empties the cache. */
  generation: 0,
});

const cache = new Map<string, Promise<unknown>>();

/** Forget everything of the previous package. */
export function resetInsights() {
  cache.clear();
  insights.info = null;
  insights.infoError = null;
  insights.scope = emptyScope();
  insights.generation += 1;
}

export async function loadInfo() {
  if (insights.info) return;
  insights.infoError = null;
  try {
    insights.info = await api.insightsInfo();
  } catch (err) {
    insights.infoError = (err as { message?: string })?.message ?? String(err);
  }
}

/** The answer to `section` for `scope`, from the cache if asked before. */
export function ask<T>(section: string, scope: Scope, run: (scope: Scope) => Promise<T>, extra = ""): Promise<T> {
  const snapshot = $state.snapshot(scope) as Scope;
  const key = `${insights.generation}|${section}|${extra}|${JSON.stringify(snapshot)}`;
  let answer = cache.get(key) as Promise<T> | undefined;
  if (!answer) {
    answer = run(snapshot);
    cache.set(key, answer);
    answer.catch(() => cache.delete(key));
  }
  return answer;
}

/** The name of place `index`, preferring the current name from the summary
 * (open DMs lend theirs when logged in). */
export function placeName(index: number, pkg: PackageSummary | null): string {
  const place = insights.info?.places[index];
  if (!place) return "";
  return pkg?.targets.find((t) => t.target.id === place.id)?.target.name ?? place.name;
}

export function scopeIsEmpty(scope: Scope): boolean {
  return !scope.from && !scope.to && scope.places.length === 0 && scope.channels.length === 0;
}
