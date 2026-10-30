<script lang="ts">
  import { onMount } from "svelte";
  import { open, save } from "@tauri-apps/plugin-dialog";
  import { DrawioBridge, pngDataUriToBase64 } from "$lib/canvas/bridge";
  import { onAiEvents, submitReply, exportFile, exportPdf, fileOpen, fileSave, settingsGet } from "$lib/aiClient";
  import { getDrawioXml, setDrawioXml, reloadTick } from "$lib/stores/canvas.svelte";
  import { t } from "$lib/stores/i18n";

  let frame: HTMLIFrameElement;
  let bridge: DrawioBridge | undefined;
  let ready = $state(false);
  let exporting = $state(false);
  // FR-X3：导出默认参数（scale/border）来自 settings.export。
  let exportScale = 2;
  let exportBorder = 20;

  // 版本恢复/外部加载触发：把当前 store 里的 XML 重新载入 iframe。
  $effect(() => {
    void reloadTick.n;
    if (!bridge || !ready) return;
    const xml = getDrawioXml();
    if (xml) bridge.load(xml);
  });

  onMount(async () => {
    try {
      const s = await settingsGet();
      exportScale = s.export.scale || 2;
      exportBorder = s.export.border ?? 20;
    } catch {
      // 读取失败用默认值
    }
    bridge = new DrawioBridge(frame, "/drawio/index.html");
    bridge.onInit(() => {
      ready = true;
      const cached = getDrawioXml();
      if (cached) bridge?.load(cached);
    });
    bridge.onSave((xml) => setDrawioXml(xml));

    onAiEvents(async (event, payload) => {
      const p = payload as Record<string, unknown>;
      if (event === "ai_tool_result") {
        bridge?.load(p.xml as string);
      } else if (event === "ai_request") {
        const kind = p.kind as string;
        const turn_id = p.turn_id as string;
        try {
          if (kind === "current_xml") {
            const xml = await bridge?.getXml();
            await submitReply(turn_id, "current_xml", { xml });
          } else if (kind === "screenshot") {
            const png = await bridge?.export("png", { scale: 1, border: 20 });
            if (png) await submitReply(turn_id, "screenshot", { png_base64: pngDataUriToBase64(png) });
          }
        } catch (e) {
          // 桥接失败静默降级（AI 回合会超时）
          console.warn("bridge request failed:", e);
        }
      }
    });
  });

  async function doExport(format: "png" | "svg" | "xml" | "visio-svg" | "pdf") {
    if (!bridge || exporting) return;
    exporting = true;
    try {
      const labels: Record<string, { name: string; ext: string }> = {
        png: { name: "PNG", ext: "png" },
        svg: { name: "SVG", ext: "svg" },
        xml: { name: "draw.io", ext: "drawio" },
        "visio-svg": { name: "Visio SVG", ext: "svg" },
        pdf: { name: "PDF", ext: "pdf" },
      };
      const { name, ext } = labels[format];
      const path = await save({ filters: [{ name, extensions: [ext] }], defaultPath: `diagram.${ext}` });
      if (!path) return;

      if (format === "pdf") {
        const svgDataUri = await bridge.export("svg", { border: exportBorder });
        const svg = decodeURIComponent(escape(atob(svgDataUri.split(",")[1] ?? "")));
        await exportPdf(path, svg, exportScale, exportBorder);
      } else if (format === "visio-svg") {
        const svgDataUri = await bridge.export("svg", { border: exportBorder });
        const svg = decodeURIComponent(escape(atob(svgDataUri.split(",")[1] ?? "")));
        await exportFile("visio-svg", path, btoa(unescape(encodeURIComponent(svg))));
      } else if (format === "xml") {
        const xml = await bridge.getXml();
        await exportFile("xml", path, btoa(unescape(encodeURIComponent(xml))));
      } else {
        const dataUri = await bridge.export(format, { scale: exportScale, border: exportBorder });
        await exportFile(format, path, dataUri.split(",")[1] ?? "");
      }
    } catch (e) {
      console.error("export failed:", e);
    } finally {
      exporting = false;
    }
  }

  function doLoad() {
    const xml = getDrawioXml();
    if (xml) bridge?.load(xml);
  }

  async function doOpen() {
    if (!bridge) return;
    const path = await open({
      multiple: false,
      directory: false,
      filters: [{ name: "draw.io", extensions: ["drawio", "xml"] }],
    });
    if (!path || typeof path !== "string") return;
    try {
      const { xml } = await fileOpen(path);
      setDrawioXml(xml);
      bridge.load(xml);
    } catch (e) {
      console.error("open failed:", e);
    }
  }

  async function doSave() {
    if (!bridge) return;
    const path = await save({
      filters: [{ name: "draw.io", extensions: ["drawio"] }],
      defaultPath: "diagram.drawio",
    });
    if (!path) return;
    try {
      const xml = await bridge.getXml();
      await fileSave(path, xml);
      setDrawioXml(xml);
    } catch (e) {
      console.error("save failed:", e);
    }
  }
</script>

<div class="relative h-full w-full">
  <iframe
    bind:this={frame}
    title="draw.io canvas"
    class="h-full w-full border-0"
  ></iframe>

  {#if !ready}
    <div class="absolute inset-0 grid place-items-center bg-surface/80 text-soft">
      {t("canvas.loading")}
    </div>
  {/if}

  <div class="absolute right-2 top-2 flex gap-1">
    <button class="chip" onclick={() => doOpen()}>{t("canvas.open")}</button>
    <button class="chip" onclick={() => doSave()}>{t("canvas.save")}</button>
    <button class="chip" onclick={() => doLoad()}>{t("canvas.refresh")}</button>
    <button class="chip" onclick={() => doExport("png")} disabled={exporting}>PNG</button>
    <button class="chip" onclick={() => doExport("svg")} disabled={exporting}>SVG</button>
    <button class="chip" onclick={() => doExport("xml")} disabled={exporting}>.drawio</button>
    <button class="chip" onclick={() => doExport("visio-svg")} disabled={exporting}>Visio</button>
    <button class="chip" onclick={() => doExport("pdf")} disabled={exporting}>PDF</button>
  </div>
</div>

<style>
  .chip {
    padding: 4px 10px;
    font-size: 12px;
    border-radius: 6px;
    background: var(--color-panel);
    border: 1px solid var(--color-outline);
    color: var(--color-ink);
    cursor: pointer;
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
