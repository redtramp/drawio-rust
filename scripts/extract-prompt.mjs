#!/usr/bin/env node
// 从 next-ai-draw-io 的 system-prompts.ts 提取系统提示词三部分为纯文本资产。
// 用法: node scripts/extract-prompt.mjs <next-ai-draw-io-repo> <out-dir>
// 依赖 node >= 23.6 的 TS type stripping（node 26 默认启用）。
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { pathToFileURL } from "node:url";
import { join, resolve } from "node:path";

const repo = resolve(process.argv[2]);
const outDir = resolve(process.argv[3]);

const srcPath = join(repo, "lib/system-prompts.ts");
const src = readFileSync(srcPath, "utf8");

// 把 @/ 别名替换为绝对路径（node 无法解析 TS 路径别名）
const absExamples = join(repo, "packages/mcp-server/src/xml-examples.ts");
const patched =
  src.replace(/@\/packages\/mcp-server\/src\/xml-examples\.ts/g, absExamples) +
  "\nexport { DEFAULT_SYSTEM_PROMPT, STYLE_INSTRUCTIONS, MINIMAL_STYLE_INSTRUCTION };\n";

const tmp = join(process.env.TMPDIR || "/tmp", `system-prompts-extract-${process.pid}.ts`);
writeFileSync(tmp, patched);

const mod = await import(`${pathToFileURL(tmp).href}?t=${Date.now()}`);

mkdirSync(outDir, { recursive: true });
writeFileSync(join(outDir, "default.md"), mod.DEFAULT_SYSTEM_PROMPT);
writeFileSync(join(outDir, "style.md"), mod.STYLE_INSTRUCTIONS);
writeFileSync(join(outDir, "minimal.md"), mod.MINIMAL_STYLE_INSTRUCTION);
console.log(
  `wrote default.md (${mod.DEFAULT_SYSTEM_PROMPT.length} chars), style.md, minimal.md -> ${outDir}`,
);
