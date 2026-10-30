// draw.io iframe postMessage 协议封装（docs/04 §3）。
// 通信双方：宿主页面（本 app）与 vendored draw.io webapp（/drawio/index.html）。

export type DrawioFormat = "xmlsvg" | "png" | "svg" | "xml";

export interface ExportOptions {
  scale?: number;
  border?: number;
  background?: string;
  transparent?: boolean;
}

interface DrawioMessage {
  event?: string;
  format?: string;
  data?: string;
  xml?: string;
  message?: string;
}

export class DrawioBridge {
  private iframe: HTMLIFrameElement;
  private pending = new Map<string, (data: string) => void>();
  private onInitCallbacks: Array<() => void> = [];
  private onSaveCallbacks: Array<(xml: string) => void> = [];
  private onErrorCallbacks: Array<(msg: string) => void> = [];
  private currentXml: string | null = null;

  constructor(iframe: HTMLIFrameElement, src: string) {
    this.iframe = iframe;
    window.addEventListener("message", (ev) => this.handle(ev));
    iframe.src = src;
  }

  private handle(ev: MessageEvent) {
    if (ev.source !== this.iframe.contentWindow) return;
    let msg: DrawioMessage;
    try {
      msg = JSON.parse(ev.data as string);
    } catch {
      return;
    }
    switch (msg.event) {
      case "init":
        this.onInitCallbacks.forEach((cb) => cb());
        break;
      case "save":
        this.currentXml = msg.xml ?? null;
        this.onSaveCallbacks.forEach((cb) => cb(msg.xml ?? ""));
        break;
      case "export": {
        const key = msg.format ?? "";
        const resolve = this.pending.get(key);
        if (resolve) {
          this.pending.delete(key);
          resolve(msg.data ?? "");
        }
        break;
      }
      case "error":
        this.onErrorCallbacks.forEach((cb) => cb(msg.message ?? "draw.io error"));
        break;
      case "exit":
        this.currentXml = msg.xml ?? null;
        break;
    }
  }

  private post(action: string, extra: Record<string, unknown> = {}) {
    this.iframe.contentWindow?.postMessage(JSON.stringify({ action, ...extra }), "*");
  }

  load(xml: string) {
    this.currentXml = xml;
    this.post("load", { xml, autosave: 1 });
  }

  /** 同源直控改画布后同步缓存（避免 getXml 返回旧值）。 */
  setCachedXml(xml: string) {
    this.currentXml = xml;
  }

  export(format: DrawioFormat, opts: ExportOptions = {}): Promise<string> {
    return new Promise((resolve) => {
      this.pending.set(format, resolve);
      this.post("export", { format, ...opts });
    });
  }

  async getXml(): Promise<string> {
    if (this.currentXml) return this.currentXml;
    const dataUri = await this.export("xmlsvg");
    return decodeXmlSvg(dataUri);
  }

  getCachedXml(): string | null {
    return this.currentXml;
  }

  onInit(cb: () => void) {
    this.onInitCallbacks.push(cb);
  }
  onSave(cb: (xml: string) => void) {
    this.onSaveCallbacks.push(cb);
  }
  onError(cb: (msg: string) => void) {
    this.onErrorCallbacks.push(cb);
  }
}

/// 从 .drawio.svg 的 dataURI 提取内嵌 XML。
export function decodeXmlSvg(dataUri: string): string {
  const base64 = dataUri.split(",")[1] ?? "";
  const svg = atob(base64);
  // <svg ... content="&lt;mxfile...&gt;"> → 提取 content 属性（HTML 实体）
  const m = svg.match(/content="([^"]*)"/);
  if (!m) return "";
  return m[1]
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&quot;/g, '"')
    .replace(/&amp;/g, "&");
}

/// 把 base64 PNG dataURI 转纯 base64（供 Rust 解码）。
export function pngDataUriToBase64(dataUri: string): string {
  return dataUri.split(",")[1] ?? "";
}

// ---------------------------------------------------------------------------
// 同源桥接（FR-E4 单步 undo / FR-E6 选中读取 / FR-A8 高亮）。
//
// vendored draw.io 与宿主同源（public/drawio），完整 app 模式下 EditorUi /
// App / mxUtils / ReplaceDiagram / mxCellHighlight 都是顶层全局符号，可直接
// 访问 iframe 内部实例。跨域或脚本未就绪时所有方法静默降级，主通道仍走
// postMessage（DrawioBridge）。
// ---------------------------------------------------------------------------

export interface SelectedCell {
  id: string;
  label: string;
  isEdge: boolean;
  source?: string;
  target?: string;
}

/**
 * draw.io EditorUi 同源直控。实例通过 hook `EditorUi.prototype` 的
 * updateActionStates / fileLoaded 方法捕获（上游 next-ai-draw-io 同款策略）。
 */
export class SameOriginBridge {
  private ui: any = null;
  private win: any = null;
  private highlights: any[] = [];
  private onSelectionCb: (() => void) | null = null;
  private _selectionHooked = false;

  /**
   * 探测同源并 hook EditorUi 实例。返回是否探测到同源（不代表已拿到实例；
   * 实例在后续 updateActionStates 调用时经 onFound 回调）。
   */
  watchForEditorUi(frameWindow: Window, onFound: (ui: any) => void): boolean {
    let w: any;
    try {
      w = frameWindow as any;
      void w.document; // 跨域会抛
    } catch {
      return false;
    }
    const hook = (proto: any, method: string) => {
      if (!proto || typeof proto[method] !== "function") return;
      if ((proto[method] as any).__naiWrapped) return;
      const original = proto[method];
      const wrapped = function (this: any, ...args: any[]) {
        onFound(this);
        return original.apply(this, args);
      };
      (wrapped as any).__naiWrapped = true;
      proto[method] = wrapped;
    };
    hook(w.EditorUi?.prototype, "updateActionStates");
    hook(w.App?.prototype, "fileLoaded");
    hook(w.EditorUi?.prototype, "fileLoaded");
    return true;
  }

  /** 拿到实例后绑定 window 引用。 */
  attach(ui: any, win: Window) {
    this.ui = ui;
    this.win = win as any;
    // 监听选中变化（graph selection model 的 CHANGE 事件）。
    try {
      const g = ui?.editor?.graph;
      const sm = g?.getSelectionModel?.();
      const mxEvent = (win as any)?.mxEvent;
      if (sm?.addListener && mxEvent?.CHANGE && !this._selectionHooked) {
        this._selectionHooked = true;
        sm.addListener(mxEvent.CHANGE, () => this.onSelectionCb?.());
      }
    } catch {
      // 忽略：selection 监听失败不影响主通道
    }
  }

  /** 选中变化回调（FR-E6 采集）。 */
  onSelectionChange(cb: () => void) {
    this.onSelectionCb = cb;
  }

  /**
   * 同源导出 SVG（完整 app 模式下 postMessage 导出通道不响应，需直控 graph）。
   * 返回完整 SVG XML 字符串；失败返回 null（调用方降级 postMessage）。
   */
  exportSvg(scale = 1, border = 0): string | null {
    const g = this.graph();
    const win = this.win;
    if (!g || !win || typeof g.getSvg !== "function" || typeof win.mxUtils?.getXml !== "function") {
      return null;
    }
    try {
      let bg = g.background;
      if (bg === win.mxConstants?.NONE) bg = null;
      const svgRoot = g.getSvg(bg, scale, border);
      if (!svgRoot) return null;
      return win.mxUtils.getXml(svgRoot);
    } catch {
      return null;
    }
  }

  hasEditor(): boolean {
    return this.ui != null;
  }

  /**
   * 在 draw.io 原菜单基础上注入自定义菜单项（FR-菜单扩展）：
   * - 顶级 "AI" 菜单，点击子项触发宿主回调（聚焦聊天/生成入口）。
   * - 在 File → Export as 末尾追加 "Visio" 导出项。
   * 通过包装 exportAs 菜单的 funct 实现，不改 vendored 源码。
   */
  installMenuExtensions(handlers: { onAi: () => void; onVisio: () => void }): boolean {
    const { ui } = this;
    if (!ui?.menubar || !ui?.menus) return false;
    try {
      // 顶级 AI 菜单（追加到菜单栏末尾，保留原 file/edit/view/arrange/extras/help）。
      // menubar.addMenu 返回的元素 class 是 geMenubar（容器样式），原菜单项靠
      // menuCreated 改成 geItem；这里手动对齐，避免绝对定位把菜单栏布局打乱。
      const aiElt = ui.menubar.addMenu("AI", (menu: any, parent: any) => {
        menu.addItem("生成图表", null, () => handlers.onAi(), parent);
      });
      if (aiElt) aiElt.className = "geItem";

      // Visio 导出：包装 exportAs.funct，在原导出项后追加。
      const exportAs = ui.menus.get("exportAs");
      if (exportAs?.funct) {
        const orig = exportAs.funct;
        exportAs.funct = function (this: unknown, menu: any, parent: any) {
          orig(menu, parent);
          menu.addItem("Visio (.vsdx)", null, () => handlers.onVisio(), parent);
        };
      }
      return true;
    } catch {
      return false;
    }
  }

  private graph(): any {
    return this.ui?.editor?.graph ?? null;
  }

  /** FR-E6：当前选中 cell 的 id 列表（跨域/无实例返回空）。 */
  readSelection(): SelectedCell[] {
    const g = this.graph();
    if (!g) return [];
    const out: SelectedCell[] = [];
    for (const cell of (g.getSelectionCells() as any[]) ?? []) {
      if (!cell?.id) continue;
      const item: SelectedCell = {
        id: String(cell.id),
        label: cell.value ? String(cell.value) : "",
        isEdge: !!g.model?.isEdge?.(cell),
      };
      if (item.isEdge) {
        const src = g.model?.getTerminal?.(cell, true);
        const tgt = g.model?.getTerminal?.(cell, false);
        if (src?.id) item.source = String(src.id);
        if (tgt?.id) item.target = String(tgt.id);
      }
      out.push(item);
    }
    return out;
  }

  private currentPageId(): string | null {
    const id = this.ui?.currentPage?.getId?.();
    return id === undefined || id === null ? null : String(id);
  }

  /** 从完整 mxfile XML 提取当前页的 <mxGraphModel>。 */
  private pageModelOf(xml: string): string | null {
    try {
      const doc = new DOMParser().parseFromString(xml, "text/xml");
      if (doc.querySelector("parsererror")) return null;
      const root = doc.documentElement;
      if (root.nodeName === "mxGraphModel") return xml;
      if (root.nodeName !== "mxfile") return null;
      const diagrams = Array.from(root.getElementsByTagName("diagram"));
      const id = this.currentPageId();
      const diagram =
        diagrams.length === 1 ? diagrams[0] : diagrams.find((d) => d.getAttribute("id") === id);
      if (!diagram) return null;
      const model = diagram.getElementsByTagName("mxGraphModel")[0];
      return model ? new XMLSerializer().serializeToString(model) : null;
    } catch {
      return null;
    }
  }

  /**
   * FR-E4：以单步 undo 提交 AI 改动（ReplaceDiagram）。失败返回 false，
   * 调用方降级到 postMessage load。
   */
  commitDiagram(xml: string): boolean {
    const { ui, win } = this;
    if (!ui || !win) return false;
    const ReplaceDiagram = win.ReplaceDiagram;
    const g = this.graph();
    const model = g?.model;
    if (typeof ReplaceDiagram !== "function" || typeof win.mxUtils?.parseXml !== "function" || !model) {
      return false;
    }
    const pageModel = this.pageModelOf(xml);
    if (!pageModel) return false;
    model.beginUpdate();
    try {
      const change = new ReplaceDiagram(ui, win.mxUtils.parseXml(pageModel).documentElement);
      model.execute(change);
    } catch {
      return false;
    } finally {
      model.endUpdate();
    }
    return true;
  }

  /** FR-A8：高亮 touched cells（画布覆盖层，非文档内容）。 */
  highlightCells(ids: string[], color = "#a86b00"): void {
    this.clearHighlights();
    const g = this.graph();
    const win = this.win;
    if (!g || typeof win?.mxCellHighlight !== "function" || ids.length === 0) return;
    for (const id of ids.slice(0, 200)) {
      const cell = g.model?.getCell?.(id);
      const state = cell ? g.view?.getState?.(cell) : null;
      if (!state) continue;
      const h = new win.mxCellHighlight(g, color, 8);
      h.opacity = 40;
      h.highlight(state);
      this.highlights.push(h);
    }
  }

  clearHighlights(): void {
    for (const h of this.highlights) {
      try {
        h.destroy();
      } catch {
        // ignore
      }
    }
    this.highlights = [];
  }
}
