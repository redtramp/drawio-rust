<script lang="ts">
  import { onMount } from "svelte";
  import {
    versionsList,
    versionRestore,
    onVersionNew,
    type VersionSnapshot,
  } from "$lib/aiClient";
  import { setDrawioXml, requestReload } from "$lib/stores/canvas.svelte";
  import { t } from "$lib/stores/i18n";

  let versions = $state<VersionSnapshot[]>([]);
  let restoring = $state(false);
  let error = $state<string | null>(null);

  async function refresh() {
    versions = await versionsList();
  }

  onMount(async () => {
    await refresh();
    onVersionNew(async () => {
      await refresh();
    });
  });

  function labelText(v: VersionSnapshot): string {
    if (v.fromScratch) return t("versions.initial");
    return v.label === "edit_diagram" ? t("versions.edit") : t("versions.generate");
  }

  function diffText(v: VersionSnapshot): string {
    const s = v.summary;
    const parts: string[] = [];
    if (s.shapesAdded) parts.push(`+${s.shapesAdded} 形`);
    if (s.shapesRemoved) parts.push(`-${s.shapesRemoved} 形`);
    if (s.shapesChanged) parts.push(`~${s.shapesChanged} 形`);
    if (s.edgesAdded) parts.push(`+${s.edgesAdded} 边`);
    if (s.edgesRemoved) parts.push(`-${s.edgesRemoved} 边`);
    if (s.edgesChanged) parts.push(`~${s.edgesChanged} 边`);
    return parts.length ? parts.join(" ") : t("versions.unchanged");
  }

  async function restore(v: VersionSnapshot) {
    if (restoring) return;
    restoring = true;
    error = null;
    try {
      const xml = await versionRestore(v.id);
      setDrawioXml(xml);
      requestReload();
    } catch (e) {
      error = String(e);
    } finally {
      restoring = false;
    }
  }
</script>

<div class="space-y-1 border-b border-outline p-2">
  <div class="flex items-center justify-between px-1">
    <span class="text-xs font-semibold text-soft">{t("versions.title")}</span>
    <button class="text-xs text-accent" onclick={refresh}>{t("versions.refresh")}</button>
  </div>

  {#if error}
    <p class="px-1 text-xs text-err">{error}</p>
  {/if}

  {#if versions.length === 0}
    <p class="px-1 text-xs text-soft">{t("versions.empty")}</p>
  {/if}

  {#each versions as v}
    <div class="flex items-center justify-between gap-2 rounded-md border border-outline bg-surface px-2 py-1">
      <div class="min-w-0">
        <span class="text-xs font-medium">#{v.seq} {labelText(v)}</span>
        <span class="ml-1 text-[11px] text-soft">{diffText(v)}</span>
      </div>
      <button class="chip" onclick={() => restore(v)} disabled={restoring}>{t("versions.restore")}</button>
    </div>
  {/each}
</div>

<style>
  .chip {
    padding: 2px 8px;
    font-size: 11px;
    border-radius: 5px;
    background: var(--color-panel);
    border: 1px solid var(--color-outline);
    color: var(--color-ink);
    cursor: pointer;
    white-space: nowrap;
  }
  .chip:hover:not(:disabled) {
    background: var(--color-accent);
    color: white;
  }
  .chip:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
</style>
