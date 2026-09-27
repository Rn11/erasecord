// Choosing the passphrase of an encrypted backup or export. Kept in memory
// only: never in presets, settings or the saved progress of a run.

import { t } from "./i18n.svelte";

export type PassphraseMode = "generated" | "own" | "file";

export interface PassphraseChoice {
  mode: PassphraseMode;
  /** The generated words, separated by spaces. */
  words: string;
  /** Which words (from 1) must be typed to confirm they were saved. */
  checks: number[];
  answers: string[];
  own: string;
  repeat: string;
  file: string | null;
}

export function newChoice(words: string): PassphraseChoice {
  const count = words.split(" ").length;
  const positions = new Set<number>();
  const random = new Uint32Array(8);
  crypto.getRandomValues(random);
  for (const r of random) {
    if (positions.size === 3) break;
    positions.add(1 + (r % count));
  }
  const checks = [...positions].sort((a, b) => a - b);
  return { mode: "generated", words, checks, answers: checks.map(() => ""), own: "", repeat: "", file: null };
}

/** A rough estimate of how hard a passphrase is to guess, in bits. */
export function strength(passphrase: string): number {
  if (!passphrase) return 0;
  let pool = 0;
  if (/[a-z]/.test(passphrase)) pool += 26;
  if (/[A-Z]/.test(passphrase)) pool += 26;
  if (/[0-9]/.test(passphrase)) pool += 10;
  if (/[^a-zA-Z0-9\s]/.test(passphrase)) pool += 33;
  if (/[^\x00-\x7f]/.test(passphrase)) pool += 100;
  if (/\s/.test(passphrase)) pool += 1;
  const unique = new Set(passphrase).size;
  // People pick patterns and words, not random letters: count about half.
  const chars = Math.min(passphrase.length, unique * 2) * Math.log2(Math.max(pool, 2)) * 0.5;
  const words = passphrase.trim().split(/\s+/).filter((w) => w.length >= 3).length;
  return Math.round(Math.max(chars, words >= 4 ? words * 10 : 0));
}

export type Strength = "weak" | "fair" | "strong";

export function strengthLevel(bits: number): Strength {
  return bits < 45 ? "weak" : bits < 70 ? "fair" : "strong";
}

/** Why the choice cannot be used yet, or null. */
export function passphraseProblem(choice: PassphraseChoice): string | null {
  switch (choice.mode) {
    case "generated": {
      const words = choice.words.split(" ");
      const ok = choice.checks.every((n, i) => choice.answers[i].trim().toLowerCase() === words[n - 1]);
      return ok ? null : t("pass.confirmNeeded");
    }
    case "own":
      if (!choice.own) return t("pass.ownNeeded");
      if (strengthLevel(strength(choice.own)) === "weak") return t("pass.tooWeak");
      if (choice.repeat !== choice.own) return t("pass.mismatch");
      return null;
    case "file":
      return choice.file ? null : t("pass.fileNeeded");
  }
}

/** What the backend needs. */
export function passphraseInput(choice: PassphraseChoice): { text?: string; file?: string } {
  switch (choice.mode) {
    case "generated":
      return { text: choice.words };
    case "own":
      return { text: choice.own };
    case "file":
      return { file: choice.file ?? undefined };
  }
}
