// Checking for a new version: the newest GitHub release's latest.json,
// whose installers are signed; the updater refuses anything not signed
// with EraseCord's key. Nothing is installed without a click.

import { isTauri } from "@tauri-apps/api/core";
import { check, type Update } from "@tauri-apps/plugin-updater";

const SKIP_KEY = "erasecord.skipVersion";

export async function findUpdate(): Promise<Update | null> {
  if (!isTauri() && !import.meta.env.DEV) return null;
  try {
    const update = await check();
    if (!update) return null;
    let skipped: string | null = null;
    try {
      skipped = localStorage.getItem(SKIP_KEY);
    } catch {
      // No storage: nothing skipped.
    }
    return skipped === update.version ? null : update;
  } catch {
    // Offline, or GitHub not reachable: try again next start.
    return null;
  }
}

export function skipVersion(version: string) {
  try {
    localStorage.setItem(SKIP_KEY, version);
  } catch {
    // Not remembered; asked again next start.
  }
}
