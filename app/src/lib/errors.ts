// Error messages from the Rust side, in the user's language where the kind
// of error is known.

import { asCommandError } from "./api";
import { t } from "./i18n.svelte";

export function errorMessage(err: unknown): string {
  const error = asCommandError(err);
  switch (error.kind) {
    case "unauthorized":
      return t("error.badToken");
    case "busy":
      return t("error.busy");
    default:
      return error.message;
  }
}
