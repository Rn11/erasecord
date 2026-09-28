<script lang="ts">
  import { filterLines } from "$lib/format";
  import { t } from "$lib/i18n.svelte";
  import type { Filter } from "$lib/types";

  // The last question before deleting.
  let {
    count,
    upTo,
    places,
    filter,
    onConfirm,
  }: {
    count: number;
    upTo: boolean;
    places: string;
    filter: Filter;
    onConfirm: () => void;
  } = $props();

  let dialog: HTMLDialogElement | undefined = $state();
  const conditions = $derived(filterLines(filter));

  export function open() {
    dialog?.showModal();
  }

  function confirm() {
    dialog?.close();
    onConfirm();
  }
</script>

<dialog bind:this={dialog} class="card confirm" aria-labelledby="confirm-title">
  <h2 id="confirm-title">{t(upTo ? "confirm.titleUpTo" : "confirm.title", { count })}</h2>
  <ul class="conditions small">
    {#each conditions as line, i (i)}<li>{line}</li>{/each}
  </ul>
  <p>{t("confirm.body", { places })} <strong>{t("confirm.undo")}</strong></p>
  <p class="muted small">{t("confirm.slow")}</p>
  <div class="actions">
    <button class="btn" onclick={() => dialog?.close()}>{t("common.cancel")}</button>
    <button class="btn danger" onclick={confirm}>{t("confirm.go")}</button>
  </div>
</dialog>

<style>
  .conditions {
    margin: 0;
    padding-left: 18px;
  }

  .confirm {
    width: min(440px, calc(100% - 32px));
    /* WebKitGTK would otherwise stretch the modal to the full window height. */
    height: fit-content;
    max-height: calc(100vh - 32px);
    padding: 22px;
    color: var(--text);
    border: 1px solid var(--border);
  }

  .confirm[open] {
    display: grid;
    align-content: start;
    gap: 12px;
  }

  .confirm::backdrop {
    background: rgb(0 0 0 / 0.45);
  }

  h2,
  p {
    margin: 0;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 6px;
  }
</style>
