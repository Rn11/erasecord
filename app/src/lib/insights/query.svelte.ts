// Loads an Insights answer for the current scope and reloads it when the
// scope changes. The previous answer stays visible (dimmed) meanwhile.

import { errorMessage } from "$lib/errors";
import { ask, insights } from "./store.svelte";
import type { Scope } from "./types";

export function useQuery<T>(
  section: string,
  scope: () => Scope,
  run: (scope: Scope) => Promise<T>,
  extra: () => string = () => "",
) {
  const state = $state({ data: null as T | null, loading: true, error: null as string | null });
  $effect(() => {
    const current = scope();
    const key = extra();
    // Track every field, and the package.
    JSON.stringify(current);
    void insights.generation;
    let live = true;
    state.loading = true;
    state.error = null;
    ask(section, current, run, key).then(
      (data) => {
        if (!live) return;
        state.data = data;
        state.loading = false;
      },
      (err) => {
        if (!live) return;
        state.error = errorMessage(err);
        state.loading = false;
      },
    );
    return () => {
      live = false;
    };
  });
  return state;
}
