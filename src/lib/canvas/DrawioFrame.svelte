<script lang="ts">
  import { onMount } from "svelte";
  import { save } from "@tauri-apps/plugin-dialog";
  import { DrawioBridge, SameOriginBridge, pngDataUriToBase64 } from "$lib/canvas/bridge";
  import { onAiEvents, submitReply, exportFile, settingsGet } from "$lib/aiClient";
  import { getDrawioXml, setDrawioXml, reloadTick, setSelectedIds } from "$lib/stores/canvas.svelte";
  import { t } from "$lib/stores/i18n.svelte";

  let frame: HTMLIFrameElement;
  let bridge: DrawioBridge | undefined;
  let sameOrigin = new SameOriginBridge();
  let ready = $state(false);
  let exporting = $state(false);
  // Visio 导出默认 border 来自 settings.export。
  let exportBorder = 20;

  // 版本恢复/外部加载触发：把当前 store 里的 XML 重新载入 iframe。
  $effect(() => {
    void reloadTick.n;
    if (!bridge || !ready) return;
    const xml = getDrawioXml();
    if (xml) loadXml(xml);
  });

  // 完整 app 模式下 postMessage 不响应，优先同源 ReplaceDiagram，失败降级 postMessage。
  function loadXml(xml: string) {
    if (sameOrigin.commitDiagram(xml)) {
      bridge?.setCachedXml(xml);
      setDrawioXml(xml);
    } else {
      bridge?.load(xml);
    }
  }

  onMount(async () => {
    try {
      const s = await settingsGet();
      exportBorder = s.export.border ?? 20;
    } catch {
      // 读取失败用默认值
    }
    bridge = new DrawioBridge(frame, "/drawio/index.html");
    // 完整 app 模式下 draw.io 不安装 installMessageHandler（见 App.js:4028 的
    // client==1 前置），因此不发 init 也不响应 postMessage load/export。
    // ready 改由同源检测驱动；onInit 仅作为 embed 模式兜底。
    bridge.onInit(() => {
      if (!ready) {
        ready = true;
        const cached = getDrawioXml();
        if (cached) bridge?.load(cached);
      }
    });
    bridge.onSave((xml) => setDrawioXml(xml));
    // 同源检测驱动 ready：完整 app 模式下依赖 EditorUi 全局符号就绪。
    startSameOriginWatch();

    onAiEvents(async (event, payload) => {
      const p = payload as Record<string, unknown>;
      if (event === "ai_tool_result") {
        const xml = p.xml as string;
        const touched = (p.touched_ids as string[]) ?? [];
        // FR-E4：优先单步 undo（ReplaceDiagram）；失败降级 postMessage 整段 load
        if (sameOrigin.commitDiagram(xml)) {
          bridge?.setCachedXml(xml);
          setDrawioXml(xml);
        } else {
          bridge?.load(xml);
        }
        // FR-A8：高亮本次改动触及的 cell
        sameOrigin.highlightCells(touched);
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

  // 轮询 draw.io 的 EditorUi 就绪后，hook 拿实例并采集选中（FR-E6）。
  function startSameOriginWatch() {
    const startedAt = Date.now();
    const poll = setInterval(() => {
      const fw = frame?.contentWindow;
      if (!fw) return;
      let hasEditor = false;
      try {
        hasEditor = fw.location.href !== "about:blank" && !!(fw as any).EditorUi;
      } catch {
        clearInterval(poll); // 跨域，放弃同源桥接
        return;
      }
      if (hasEditor) {
        clearInterval(poll);
        // EditorUi 已就绪 → 完整 app 菜单栏已渲染，撤下 loading 遮罩。
        if (!ready) ready = true;
        sameOrigin.watchForEditorUi(fw, (ui) => {
          if (sameOrigin.hasEditor()) return;
          sameOrigin.attach(ui, fw);
          // 注入 Visio 导出项（保留原 draw.io 菜单）。
          sameOrigin.installMenuExtensions({
            onVisio: () => exportVisio(),
          });
          // 初始缓存：完整 app 不响应 postMessage load，走同源 ReplaceDiagram。
          const cached = getDrawioXml();
          if (cached) loadXml(cached);
          sameOrigin.onSelectionChange(() => {
            setSelectedIds(sameOrigin.readSelection().map((c) => c.id));
          });
          setSelectedIds(sameOrigin.readSelection().map((c) => c.id));
        });
      } else if (Date.now() - startedAt > 60_000) {
        clearInterval(poll);
      }
    }, 50);
  }

  // Visio 导出：draw.io 原生菜单不具备，注入到 File → Export as 后触发。
  async function exportVisio() {
    if (!bridge || exporting) return;
    exporting = true;
    try {
      const path = await save({ filters: [{ name: "Visio SVG", extensions: ["svg"] }], defaultPath: "diagram.svg" });
      if (!path) return;
      const svg = sameOrigin.exportSvg(1, exportBorder) ?? (await exportSvgViaPost());
      await exportFile("visio-svg", path, btoa(unescape(encodeURIComponent(svg))));
    } catch (e) {
      console.error("export failed:", e);
    } finally {
      exporting = false;
    }
  }

  // 完整 app 模式下 postMessage 导出通道不响应；同源失败时降级 postMessage。
  async function exportSvgViaPost(): Promise<string> {
    const svgDataUri = await bridge!.export("svg", { border: exportBorder });
    return decodeURIComponent(escape(atob(svgDataUri.split(",")[1] ?? "")));
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
</div>

