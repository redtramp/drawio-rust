// Tauri 命令/事件封装（docs/04 §1/§2）。
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export interface CanvasContext {
  full_xml: string;
  page_id?: string;
  page_name?: string;
  page_count: number;
  selection: string[];
}

export async function sendChat(
  session_id: string,
  message: string,
  canvas: CanvasContext,
): Promise<string> {
  const { turn_id } = await invoke<{ turn_id: string }>("ai_chat_send", {
    sessionId: session_id,
    message,
    canvas,
  });
  return turn_id;
}

export function submitReply(
  turn_id: string,
  kind: string,
  payload: { xml?: string; png_base64?: string },
) {
  return invoke("ai_submit", {
    turnId: turn_id,
    kind,
    xml: payload.xml ?? null,
    pngBase64: payload.png_base64 ?? null,
  });
}

export function cancelTurn(turn_id: string) {
  return invoke("ai_turn_cancel", { turnId: turn_id });
}

const AI_EVENTS = [
  "ai_stream",
  "ai_tool_result",
  "ai_turn_done",
  "ai_turn_complete",
  "ai_turn_error",
  "ai_request",
] as const;

export async function onAiEvents(handler: (event: string, payload: unknown) => void): Promise<() => void> {
  const unlisteners = await Promise.all(AI_EVENTS.map((ev) => listen(ev, (e) => handler(ev, e.payload))));
  return () => unlisteners.forEach((un) => un());
}

export function providersList(): Promise<ProviderEntry[]> {
  return invoke("providers_list");
}

export interface ProviderEntry {
  id: string;
  name: string;
  protocol: string;
  default_base_url: string;
  default_model: string;
  api_key_label: string;
  supports_thinking: boolean;
  requires_api_key: boolean;
}

export function settingsGet(): Promise<Settings> {
  return invoke("settings_get");
}

export interface Settings {
  ai_enabled: boolean;
  provider: {
    provider_id: string;
    model: string;
    base_url?: string;
    validation_provider_id?: string;
    validation_model?: string;
  };
  export: { scale: number; border: number; transparent: boolean };
  ui: { lang: string; theme: string };
  sync: { auto_check: boolean };
}

export function settingsSave(s: {
  provider_id: string;
  model: string;
  base_url?: string;
  api_key?: string;
}): Promise<void> {
  return invoke("settings_save", {
    providerId: s.provider_id,
    model: s.model,
    baseUrl: s.base_url ?? null,
    validationProviderId: null,
    validationModel: null,
    apiKey: s.api_key ?? null,
  });
}

export function modelTest(s: {
  provider_id: string;
  model: string;
  base_url?: string;
  api_key?: string;
}): Promise<{ ok: boolean; latency_ms: number; message: string }> {
  return invoke("model_test", {
    providerId: s.provider_id,
    model: s.model,
    baseUrl: s.base_url ?? null,
    apiKey: s.api_key ?? null,
  });
}

export function exportFile(format: string, path: string, data: string): Promise<{ converted: number }> {
  return invoke("export_file", { format, path, data });
}

export function exportPdf(path: string, svg: string, scale?: number, border?: number): Promise<void> {
  return invoke("export_pdf", { path, svg, scale: scale ?? null, border: border ?? null });
}

export function fileOpen(path: string): Promise<{ path: string; xml: string }> {
  return invoke("file_open", { path });
}

export function fileSave(path: string, xml: string): Promise<void> {
  return invoke("file_save", { path, xml });
}

export function syncStatus(): Promise<unknown> {
  return invoke("sync_status");
}
export function syncRun(all: boolean, dryRun: boolean): Promise<unknown> {
  return invoke("sync_run", { all, dryRun });
}

export interface VersionSnapshot {
  id: string;
  label: string;
  summary: {
    shapesAdded: number;
    shapesRemoved: number;
    shapesChanged: number;
    edgesAdded: number;
    edgesRemoved: number;
    edgesChanged: number;
  };
  fromScratch: boolean;
  seq: number;
}

export function versionsList(): Promise<VersionSnapshot[]> {
  return invoke("versions_list");
}

export function versionRestore(id: string): Promise<string> {
  return invoke("version_restore", { id });
}

export async function onVersionNew(handler: (v: VersionSnapshot) => void): Promise<() => void> {
  const un = await listen("version_new", (e) => handler(e.payload as VersionSnapshot));
  return () => un();
}
