// 轻量 i18n（FR-S3：中/英）。模块级 runes store，无第三方依赖。
// 语言持久化在 settings.ui.lang，启动时从 settings 载入。

export type Lang = "zh-CN" | "en";

const DICT: Record<string, { "zh-CN": string; en: string }> = {
  "app.subtitle": { "zh-CN": "AI 绘图 · Visio 导出", en: "AI diagrams · Visio export" },
  "topbar.settings": { "zh-CN": "设置", en: "Settings" },
  "topbar.theme": { "zh-CN": "主题", en: "Theme" },
  "chat.title": { "zh-CN": "AI 助手", en: "AI Assistant" },
  "chat.tagline": { "zh-CN": "生成 / 编辑图形", en: "Generate / edit diagrams" },
  "chat.placeholder": { "zh-CN": "描述要绘制的图…", en: "Describe the diagram…" },
  "chat.empty": {
    "zh-CN": "描述你想画的图，例如“画一个用户登录流程的流程图”。",
    en: "Describe the diagram you want, e.g. “draw a user login flowchart”.",
  },
  "chat.send": { "zh-CN": "发送", en: "Send" },
  "chat.stop": { "zh-CN": "停止", en: "Stop" },
  "canvas.loading": { "zh-CN": "加载编辑器…", en: "Loading editor…" },
  "versions.title": { "zh-CN": "版本历史", en: "Version history" },
  "versions.refresh": { "zh-CN": "刷新", en: "Refresh" },
  "versions.empty": {
    "zh-CN": "尚无版本——发送一条绘图指令后自动记录。",
    en: "No versions yet — recorded automatically after a draw instruction.",
  },
  "versions.restore": { "zh-CN": "恢复", en: "Restore" },
  "versions.initial": { "zh-CN": "初始生成", en: "Initial" },
  "versions.edit": { "zh-CN": "编辑", en: "Edit" },
  "versions.generate": { "zh-CN": "生成", en: "Generate" },
  "versions.unchanged": { "zh-CN": "无变化", en: "No change" },
  "settings.title": { "zh-CN": "模型设置", en: "Model settings" },
  "settings.provider": { "zh-CN": "Provider", en: "Provider" },
  "settings.model": { "zh-CN": "模型", en: "Model" },
  "settings.baseUrl": { "zh-CN": "Base URL", en: "Base URL" },
  "settings.apiKey": { "zh-CN": "API Key", en: "API Key" },
  "settings.test": { "zh-CN": "测试连接", en: "Test" },
  "settings.save": { "zh-CN": "保存", en: "Save" },
  "settings.cancel": { "zh-CN": "取消", en: "Cancel" },
};

export const lang = $state<{ value: Lang }>({ value: "zh-CN" });

export function t(key: string): string {
  const entry = DICT[key];
  if (!entry) return key;
  return entry[lang.value];
}

export function setLang(l: Lang) {
  lang.value = l;
}
