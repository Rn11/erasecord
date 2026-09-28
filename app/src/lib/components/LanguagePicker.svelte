<script lang="ts">
  import { i18n, LANGUAGES, systemLocale, t, type Choice } from "$lib/i18n.svelte";

  const systemName = $derived(LANGUAGES.find((l) => l.code === systemLocale())?.name ?? "English");
</script>

<label class="language">
  <span class="sr-only">{t("common.language")}</span>
  <svg class="icon" viewBox="0 0 24 24" width="15" height="15" aria-hidden="true">
    <circle cx="12" cy="12" r="9" />
    <path d="M3 12h18M12 3c2.5 2.6 3.8 5.6 3.8 9s-1.3 6.4-3.8 9M12 3c-2.5 2.6-3.8 5.6-3.8 9s1.3 6.4 3.8 9" />
  </svg>
  <select
    value={i18n.choice}
    onchange={(event) => i18n.set(event.currentTarget.value as Choice)}
    title={t("common.language")}
  >
    <option value="system">{t("common.systemLanguage", { name: systemName })}</option>
    {#each LANGUAGES as language (language.code)}
      <option value={language.code} lang={language.code}>{language.name}</option>
    {/each}
  </select>
</label>

<style>
  .icon {
    flex: none;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.6;
    stroke-linecap: round;
  }

  .language {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 13px;
    color: var(--muted);
  }

  select {
    padding: 4px 8px;
    font-size: 13px;
  }

  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
    white-space: nowrap;
  }
</style>
