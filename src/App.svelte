<script lang="ts">
  // drawio-rust 应用外壳：左画布（vendored draw.io）+ 右聊天面板 + 顶栏。
  // 完整面板在 S2–S6 实现；此文件维护全局布局与主题/语言。
  import { onMount } from "svelte";
  import DrawioFrame from "$lib/canvas/DrawioFrame.svelte";
  import ChatPanel from "$lib/chat/ChatPanel.svelte";
  import TopBar from "$lib/TopBar.svelte";

  let dark = $state(true);

  $effect(() => {
    document.documentElement.classList.toggle("dark", dark);
  });

  onMount(() => {
    // 跟随系统主题（NFR-4 深色模式）
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    dark = mq.matches;
    mq.addEventListener("change", (e) => (dark = e.matches));
  });
</script>

<div class="flex h-full flex-col">
  <TopBar bind:dark />
  <div class="flex min-h-0 flex-1">
    <div class="min-w-0 flex-1">
      <DrawioFrame />
    </div>
    <ChatPanel />
  </div>
</div>
