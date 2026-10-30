<script lang="ts">
  import { onMount } from "svelte";
  import {
    providersList,
    settingsGet,
    settingsSave,
    modelTest,
    type ProviderEntry,
  } from "$lib/aiClient";
  import { t } from "$lib/stores/i18n";

  let { onclose }: { onclose: () => void } = $props();

  let providers = $state<ProviderEntry[]>([]);
  let providerId = $state("openai");
  let model = $state("");
  let baseUrl = $state("");
  let apiKey = $state("");
  let testing = $state(false);
  let testResult = $state<string | null>(null);

  onMount(async () => {
    providers = await providersList();
    const s = await settingsGet();
    providerId = s.provider.provider_id;
    model = s.provider.model;
    baseUrl = s.provider.base_url ?? "";
  });

  const selected = $derived(providers.find((p) => p.id === providerId));

  async function test() {
    testing = true;
    testResult = null;
    try {
      const r = await modelTest({
        provider_id: providerId,
        model,
        base_url: baseUrl || undefined,
        api_key: apiKey || undefined,
      });
      testResult = r.ok
        ? `✓ 连接成功 (${r.latency_ms}ms): ${r.message}`
        : `✗ ${r.message}`;
    } catch (e) {
      testResult = `✗ ${String(e)}`;
    } finally {
      testing = false;
    }
  }

  async function save() {
    await settingsSave({
      provider_id: providerId,
      model,
      base_url: baseUrl || undefined,
      api_key: apiKey || undefined,
    });
    onclose();
  }
</script>

<div class="fixed inset-0 z-50 grid place-items-center bg-black/40" onclick={(e) => e.target === e.currentTarget && onclose()}>
  <div class="w-[460px] rounded-xl border border-outline bg-panel p-5 shadow-xl">
    <h2 class="mb-4 text-base font-semibold">{t("settings.title")}</h2>

    <label class="mb-2 block text-xs text-soft">{t("settings.provider")}</label>
    <select class="field mb-3" bind:value={providerId}>
      {#each providers as p}
        <option value={p.id}>{p.name}</option>
      {/each}
    </select>

    <label class="mb-2 block text-xs text-soft">{t("settings.model")}</label>
    <input class="field mb-3" bind:value={model} placeholder={selected?.default_model ?? "model"} />

    <label class="mb-2 block text-xs text-soft">{t("settings.baseUrl")}（留空用默认 {selected?.default_base_url ?? ""}）</label>
    <input class="field mb-3" bind:value={baseUrl} placeholder={selected?.default_base_url ?? ""} />

    <label class="mb-2 block text-xs text-soft">{t("settings.apiKey")}（{selected?.api_key_label ?? ""}，留空不修改）</label>
    <input class="field mb-3" type="password" bind:value={apiKey} placeholder="sk-…" />

    {#if testResult}
      <p class="mb-3 text-xs {testResult.startsWith('✓') ? 'text-ok' : 'text-err'}">{testResult}</p>
    {/if}

    <div class="flex justify-end gap-2">
      <button class="btn" onclick={() => onclose()}>{t("settings.cancel")}</button>
      <button class="btn" onclick={test} disabled={testing}>{testing ? "测试中…" : t("settings.test")}</button>
      <button class="btn-primary" onclick={save}>{t("settings.save")}</button>
    </div>
  </div>
</div>

<style>
  .field {
    width: 100%;
    padding: 6px 10px;
    border-radius: 8px;
    border: 1px solid var(--color-outline);
    background: var(--color-surface);
    color: var(--color-ink);
    font-size: 13px;
    outline: none;
  }
  .field:focus {
    border-color: var(--color-accent);
  }
</style>
