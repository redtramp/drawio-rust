<script lang="ts">
  import SettingsDialog from "$lib/settings/SettingsDialog.svelte";
  import SyncPanel from "$lib/sync/SyncPanel.svelte";

  let { dark = $bindable() }: { dark: boolean } = $props();
  let showSettings = $state(false);
  let showSync = $state(false);
</script>

<header class="flex h-11 items-center justify-between border-b border-outline bg-panel px-3">
  <div class="flex items-center gap-2">
    <span class="h-4 w-4 rounded bg-accent"></span>
    <span class="text-sm font-semibold">drawio-rust</span>
    <span class="text-xs text-soft">AI 绘图 · Visio 导出</span>
  </div>

  <div class="flex items-center gap-1">
    <button class="icon-btn" onclick={() => (showSync = true)} title="上游同步">⇄</button>
    <button class="icon-btn" onclick={() => (showSettings = true)} title="设置">⚙</button>
    <button class="icon-btn" onclick={() => (dark = !dark)} title="主题">{dark ? "☀" : "🌙"}</button>
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
    font-size: 14px;
  }
  .icon-btn:hover {
    background: var(--color-surface);
    border-color: var(--color-outline);
  }
</style>
