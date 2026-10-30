// 画布 XML 状态（iframe 与聊天面板共享）。
export const drawioXml = $state<{ value: string | null }>({ value: null });

export function getDrawioXml(): string | null {
  return drawioXml.value;
}

export function setDrawioXml(xml: string) {
  drawioXml.value = xml;
}
