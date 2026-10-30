import { vitePreprocess } from "@sveltejs/vite-plugin-svelte";

// Svelte 5 预处理配置（供 svelte-check 与 vite-plugin-svelte 共用）。
export default {
  preprocess: vitePreprocess(),
};
