<script lang="ts">
  import { onMount } from "svelte";
  import { sendChat, onAiEvents } from "$lib/aiClient";
  import { getDrawioXml } from "$lib/stores/canvas.svelte";
  import VersionCard from "$lib/chat/VersionCard.svelte";

  const BLANK =
    '<mxfile><diagram id="p1" name="Page-1"><mxGraphModel><root><mxCell id="0"/><mxCell id="1" parent="0"/></root></mxGraphModel></diagram></mxfile>';

  type Msg = { role: "user" | "assistant" | "error"; content: string };

  let messages = $state<Msg[]>([]);
  let input = $state("");
  let streaming = $state(false);
  let sessionId = "default";
  let scrollEl: HTMLDivElement;

  onMount(() => {
    onAiEvents((event, payload) => {
      const p = payload as Record<string, any>;
      if (event === "ai_stream" && p.kind === "token") {
        const last = messages[messages.length - 1];
        if (last && last.role === "assistant") {
          last.content += p.text ?? "";
        } else {
          messages = [...messages, { role: "assistant", content: p.text ?? "" }];
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
      selection: [],
    };
    try {
      await sendChat(sessionId, text, canvas);
    } catch (e) {
      streaming = false;
      messages = [...messages, { role: "error", content: String(e) }];
    }
  }

  function stop() {
    streaming = false;
  }
</script>

<div class="flex h-full w-80 flex-col border-l border-outline bg-panel">
  <div class="flex items-center justify-between border-b border-outline px-3 py-2">
    <span class="text-sm font-semibold">AI 助手</span>
    <span class="text-xs text-soft">生成 / 编辑图形</span>
  </div>

  <VersionCard />

  <div bind:this={scrollEl} class="min-h-0 flex-1 space-y-2 overflow-y-auto p-3">
    {#if messages.length === 0}
      <p class="text-xs text-soft">
        描述你想画的图，例如“画一个用户登录流程的流程图”。
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
          {m.content || "…"}
        </div>
      {/if}
    {/each}
  </div>

  <div class="flex items-center gap-2 border-t border-outline p-2">
    <input
      class="min-w-0 flex-1 rounded-md border border-outline bg-surface px-2 py-1.5 text-sm outline-none focus:border-accent"
      placeholder="描述要绘制的图…"
      bind:value={input}
      onkeydown={(e) => {
        if (e.key === "Enter" && !e.shiftKey) {
          e.preventDefault();
          send();
        }
      }}
    />
    {#if streaming}
      <button class="btn" onclick={stop}>停止</button>
    {:else}
      <button class="btn-primary" onclick={send} disabled={!input.trim()}>发送</button>
    {/if}
  </div>
</div>
