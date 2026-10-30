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
