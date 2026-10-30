<script lang="ts">
  import { onMount } from "svelte";
  import { sendChat, onAiEvents, sessionHistory, type UploadedImage } from "$lib/aiClient";
  import { getDrawioXml, getSelectedIds, chatFocusTick } from "$lib/stores/canvas.svelte";
  import { t } from "$lib/stores/i18n.svelte";
  import VersionCard from "$lib/chat/VersionCard.svelte";

  const BLANK =
    '<mxfile><diagram id="p1" name="Page-1"><mxGraphModel><root><mxCell id="0"/><mxCell id="1" parent="0"/></root></mxGraphModel></diagram></mxfile>';

  type Msg = {
    role: "user" | "assistant" | "error";
    content: string;
    thinking?: string;
    thinkingOpen?: boolean;
  };

  let messages = $state<Msg[]>([]);
  let input = $state("");
  let streaming = $state(false);
  let sessionId = "default";
  let scrollEl: HTMLDivElement;
  // FR-A7：待发送的上传图片
  let pendingImages = $state<UploadedImage[]>([]);
  let fileInput: HTMLInputElement;
  let chatInput: HTMLInputElement;

  // AI 菜单项聚焦：draw.io 内点击后自增信号，聚焦聊天输入框。
  $effect(() => {
    void chatFocusTick.n;
    queueMicrotask(() => chatInput?.focus());
  });

  onMount(async () => {
    // 会话恢复：加载历史消息
    const history = await sessionHistory(sessionId);
    if (history.length) {
      messages = history.map((h) => ({
        role: h.role === "assistant" ? "assistant" : "user",
        content: h.content,
      }));
    }

    onAiEvents((event, payload) => {
      const p = payload as Record<string, any>;
      if (event === "ai_stream" && p.kind === "token") {
        const last = messages[messages.length - 1];
        if (last && last.role === "assistant") {
          last.content += p.text ?? "";
        } else {
          messages = [...messages, { role: "assistant", content: p.text ?? "" }];
        }
      } else if (event === "ai_stream" && p.kind === "thinking") {
        // FR-A10：思考型模型的过程展示（正文前的可折叠块）
        const last = messages[messages.length - 1];
        if (last && last.role === "assistant") {
          last.thinking = (last.thinking ?? "") + (p.text ?? "");
          last.thinkingOpen = true;
        } else {
          messages = [...messages, { role: "assistant", content: "", thinking: p.text ?? "", thinkingOpen: true }];
        }
      } else if (event === "ai_turn_done") {
        streaming = false;
        if (p.text && messages.length > 0 && messages[messages.length - 1].content === "") {
          messages[messages.length - 1].content = p.text;
        }
      } else if (event === "ai_turn_complete") {
        streaming = false;
      } else if (event === "ai_turn_error") {
        streaming = false;
        messages = [...messages, { role: "error", content: p.message ?? "未知错误" }];
      }
    });
  });

  function scrollToBottom() {
    queueMicrotask(() => {
      if (scrollEl) scrollEl.scrollTop = scrollEl.scrollHeight;
    });
  }
  $effect(() => {
    void messages;
    scrollToBottom();
  });

  async function send() {
    const text = input.trim();
    if (!text || streaming) return;
    input = "";
    messages = [...messages, { role: "user", content: text }];
    streaming = true;
    const canvas = {
      full_xml: getDrawioXml() ?? BLANK,
      page_count: 1,
      selection: getSelectedIds(),
    };
    const images = pendingImages.length ? pendingImages : undefined;
    pendingImages = [];
    try {
      await sendChat(sessionId, text, canvas, images);
    } catch (e) {
      streaming = false;
      messages = [...messages, { role: "error", content: String(e) }];
    }
  }

  function stop() {
    streaming = false;
  }

  // FR-A7：选择图片文件 → base64（纯浏览器 FileReader，不新增 Tauri 依赖）。
  function pickImage() {
    fileInput?.click();
  }

  function onFilesPicked(e: Event) {
    const input = e.target as HTMLInputElement;
    const files = input.files;
    if (!files) return;
    for (const file of Array.from(files)) {
      if (!file.type.startsWith("image/")) continue;
      const reader = new FileReader();
      reader.onload = () => {
        const dataUri = reader.result as string;
        const base64 = dataUri.split(",")[1] ?? "";
        if (base64) pendingImages = [...pendingImages, { mime: file.type, data_base64: base64 }];
      };
      reader.readAsDataURL(file);
    }
    input.value = "";
  }

  function removeImage(i: number) {
    pendingImages = pendingImages.filter((_, idx) => idx !== i);
  }
</script>

<div class="flex h-full w-80 flex-col border-l border-outline bg-panel">
  <div class="flex items-center justify-between border-b border-outline px-3 py-2">
    <span class="text-sm font-semibold">{t("chat.title")}</span>
    <span class="text-xs text-soft">{t("chat.tagline")}</span>
  </div>

  <VersionCard />

  <div bind:this={scrollEl} class="min-h-0 flex-1 space-y-2 overflow-y-auto p-3">
    {#if messages.length === 0}
      <p class="text-xs text-soft">
        {t("chat.empty")}
      </p>
    {/if}
    {#each messages as m}
      {#if m.role === "user"}
        <div class="ml-auto w-fit max-w-[85%] rounded-lg bg-accent px-3 py-2 text-sm text-white">
          {m.content}
        </div>
      {:else if m.role === "error"}
        <div class="rounded-lg border border-err/40 bg-err/10 px-3 py-2 text-sm text-err">
          {m.content}
        </div>
      {:else}
        <div class="w-fit max-w-[95%] rounded-lg bg-surface px-3 py-2 text-sm">
          {#if m.thinking}
            <button
              class="mb-1 flex items-center gap-1 text-xs text-soft"
              onclick={() => (m.thinkingOpen = !m.thinkingOpen)}
              aria-expanded={m.thinkingOpen}
            >
              <span>{m.thinkingOpen ? "▾" : "▸"}</span>
              <span>思考过程</span>
            </button>
            {#if m.thinkingOpen}
              <div class="mb-2 whitespace-pre-wrap border-l-2 border-outline pl-2 text-xs text-soft">
                {m.thinking}
              </div>
            {/if}
          {/if}
          {m.content || "…"}
        </div>
      {/if}
    {/each}
  </div>

  <div class="flex flex-col gap-1 border-t border-outline p-2">
    {#if pendingImages.length > 0}
      <div class="flex flex-wrap gap-1">
        {#each pendingImages as img, i}
          <div class="relative">
            <img
              class="h-12 w-12 rounded-md border border-outline object-cover"
              src={`data:${img.mime};base64,${img.data_base64}`}
              alt=""
            />
            <button
              class="absolute -right-1 -top-1 flex h-4 w-4 items-center justify-center rounded-full bg-err text-[10px] text-white"
              onclick={() => removeImage(i)}
              aria-label="移除"
            >×</button>
          </div>
        {/each}
      </div>
    {/if}
    <div class="flex items-center gap-2">
      <input
        type="file"
        accept="image/*"
        multiple
        class="hidden"
        bind:this={fileInput}
        onchange={onFilesPicked}
      />
      <button class="icon-btn" onclick={pickImage} title="上传图片">🖼</button>
      <input
        class="min-w-0 flex-1 rounded-md border border-outline bg-surface px-2 py-1.5 text-sm outline-none focus:border-accent"
        placeholder={t("chat.placeholder")}
        bind:this={chatInput}
        bind:value={input}
        onkeydown={(e) => {
          if (e.key === "Enter" && !e.shiftKey) {
            e.preventDefault();
            send();
          }
        }}
      />
      {#if streaming}
        <button class="btn" onclick={stop}>{t("chat.stop")}</button>
      {:else}
        <button class="btn-primary" onclick={send} disabled={!input.trim()}>{t("chat.send")}</button>
      {/if}
    </div>
  </div>
</div>

<style>
  .icon-btn {
    width: 32px;
    height: 32px;
    border-radius: 6px;
    border: 1px solid transparent;
    background: transparent;
    color: var(--color-ink);
    cursor: pointer;
    font-size: 16px;
  }
  .icon-btn:hover {
    background: var(--color-surface);
    border-color: var(--color-outline);
  }
</style>
