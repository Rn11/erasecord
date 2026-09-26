<script lang="ts">
  import { i18n, LANGUAGES, systemLocale, t, type Choice } from "$lib/i18n.svelte";

  const systemName = $derived(LANGUAGES.find((l) => l.code === systemLocale())?.name ?? "English");
</script>

<label class="language">
  <span class="sr-only">{t("common.language")}</span>
  <span aria-hidden="true">🌐</span>
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
