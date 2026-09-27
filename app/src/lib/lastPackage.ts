// The data package opened last, so it can be opened again with one click.
// Only its location is kept, on this device.

export interface LastPackage {
  path: string;
  name: string;
  messages: number;
  /** When it was opened, ms since 1970. */
  at: number;
}

const KEY = "erasecord.lastPackage";

export function loadLastPackage(): LastPackage | null {
  try {
    const value = JSON.parse(localStorage.getItem(KEY) ?? "null");
    return value && typeof value.path === "string" && typeof value.name === "string" ? value : null;
  } catch {
    return null;
  }
}

export function saveLastPackage(path: string, messages: number) {
  const name = path.split(/[\\/]/).filter(Boolean).pop() ?? path;
  try {
    localStorage.setItem(KEY, JSON.stringify({ path, name, messages, at: Date.now() }));
  } catch {
    // Not remembering is fine.
  }
}

export function forgetLastPackage() {
  try {
    localStorage.removeItem(KEY);
  } catch {
    // Nothing to forget.
  }
}
