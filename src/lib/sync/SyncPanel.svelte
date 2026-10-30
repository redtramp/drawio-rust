<script lang="ts">
  import { syncStatus, syncRun } from "$lib/aiClient";
  import { t } from "$lib/stores/i18n";

  let { onclose }: { onclose: () => void } = $props();
  let report = $state<any>(null);
  let running = $state(false);
  let dryRun = $state(true);

  async function status() {
    running = true;
    try {
      report = await syncStatus();
    } catch (e) {
      report = { error: String(e) };
    } finally {
      running = false;
    }
  }

  async function run() {
    running = true;
    try {
      report = await syncRun(true, dryRun);
    } catch (e) {
      report = { error: String(e) };
    } finally {
      running = false;
    }
  }
</script>

<div
  class="fixed inset-0 z-50 grid place-items-center bg-black/40"
  role="presentation"
  onclick={(e) => e.target === e.currentTarget && onclose()}
  onkeydown={(e) => e.key === "Escape" && onclose()}
>
  <div
    class="w-[520px] rounded-xl border border-outline bg-panel p-5 shadow-xl"
    role="dialog"
    aria-modal="true"
    aria-label={t("sync.title")}
  >
    <h2 class="mb-1 text-base font-semibold">{t("sync.title")}</h2>
    <p class="mb-4 text-xs text-soft">
      检测 draw.io / next-ai-draw-io / drawio-to-visio 的漂移；L1 vendor 换版、L2 资产覆盖、
      L3 逻辑变更仅报告（永不自动合并，见 docs/06）。
    </p>

    <label class="mb-3 flex items-center gap-2 text-sm">
      <input type="checkbox" bind:checked={dryRun} />
      {t("sync.dryrun")}
    </label>

    <div class="mb-4 flex gap-2">
      <button class="btn" onclick={status} disabled={running}>{t("sync.status")}</button>
      <button class="btn-primary" onclick={run} disabled={running}>{running ? "运行中…" : t("sync.run")}</button>
    </div>

    {#if report?.error}
      <p class="text-sm text-err">{report.error}</p>
    {/if}

    {#if report?.upstreams}
      <div class="space-y-2">
        {#each report.upstreams as u}
          <div class="rounded-md border border-outline bg-surface p-2 text-xs">
            <div class="flex items-center justify-between">
              <span class="font-semibold">{u.id}</span>
              <span class={u.drift ? "text-warn" : "text-ok"}>{u.drift ? t("sync.drift") : t("sync.uptodate")}</span>
            </div>
            <div class="mt-1 text-soft">
              head {u.upstream_head?.slice(0, 8) ?? ""} ·
              L1 {u.l1_changed ? "变更" : "—"} ·
              L2 {u.l2_changed ? "变更" : "—"} ·
              L3 {u.l3_changed ? "待再移植" : "—"}
            </div>
            {#if u.l2_files?.length}
              <div class="mt-1 text-soft">{u.l2_files.length} 个资产更新</div>
            {/if}
          </div>
        {/each}
      </div>
    {/if}
  </div>
</div>
