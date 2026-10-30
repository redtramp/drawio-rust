// 画布 XML 状态（iframe 与聊天面板共享）。
export const drawioXml = $state<{ value: string | null }>({ value: null });

// 重载信号：版本恢复/外部加载时自增，DrawioFrame 监听后把当前 XML 载入 iframe。
export const reloadTick = $state<{ n: number }>({ n: 0 });

export function getDrawioXml(): string | null {
  return drawioXml.value;
}

export function setDrawioXml(xml: string) {
  drawioXml.value = xml;
}

export function requestReload() {
  reloadTick.n += 1;
}
