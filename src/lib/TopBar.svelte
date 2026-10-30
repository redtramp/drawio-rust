<script lang="ts">
  import { onMount } from "svelte";
  import SettingsDialog from "$lib/settings/SettingsDialog.svelte";
  import SyncPanel from "$lib/sync/SyncPanel.svelte";
  import { settingsGet } from "$lib/aiClient";
  import { t, lang, setLang } from "$lib/stores/i18n.svelte";

  let { dark = $bindable() }: { dark: boolean } = $props();
  let showSettings = $state(false);
  let showSync = $state(false);

  onMount(async () => {
    // 首次引导 + 载入语言
    const s = await settingsGet();
    if (s.ui.lang === "en") setLang("en");
    if (!s.onboarded) showSettings = true;
  });

  function toggleLang() {
    setLang(lang.value === "zh-CN" ? "en" : "zh-CN");
  }
</script>

<header class="flex h-11 items-center justify-between border-b border-outline bg-panel px-3">
  <div class="flex items-center gap-2">
    <span class="h-4 w-4 rounded bg-accent"></span>
    <span class="text-sm font-semibold">drawio-rust</span>
    <span class="text-xs text-soft">{t("app.subtitle")}</span>
  </div>

  <div class="flex items-center gap-1">
    <button class="icon-btn" onclick={toggleLang} title="Language">{lang.value === "zh-CN" ? "EN" : "中"}</button>
    <button class="icon-btn" onclick={() => (showSync = true)} title={t("topbar.sync")}>⇄</button>
    <button class="icon-btn" onclick={() => (showSettings = true)} title={t("topbar.settings")}>⚙</button>
    <button class="icon-btn" onclick={() => (dark = !dark)} title={t("topbar.theme")}>{dark ? "☀" : "🌙"}</button>
  </div>
</header>

{#if showSettings}
  <SettingsDialog onclose={() => (showSettings = false)} />
{/if}
{#if showSync}
  <SyncPanel onclose={() => (showSync = false)} />
{/if}

<style>
  .icon-btn {
    width: 28px;
    height: 28px;
    border-radius: 6px;
    border: 1px solid transparent;
    background: transparent;
    color: var(--color-ink);
    cursor: pointer;
    font-size: 12px;
  }
  .icon-btn:hover {
    background: var(--color-surface);
    border-color: var(--color-outline);
  }
</style>
