#!/usr/bin/env node
// W1-2 体检样例报告 · 入口
// pnpm report:sample [--no-redact] [--out output/report-sample] [--md-only]
//
// 采集（现有 CLI --json）→ 组装（九段模板 + R1-R5 硬断言）→ 渲染（MD + HTML + PDF）
// 设计与验收标准见 docs/w1-report-sample-plan.md。
// 产物目录故意不用 dist/：vite build 的 emptyOutDir 会清空 dist/（实测发生过）。

import { existsSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { collect } from "./collect.mjs";
import { assemble } from "./assemble.mjs";
import { render, printPdf } from "./render.mjs";

const ROOT = resolve(join(fileURLToPath(import.meta.url), "..", "..", ".."));
const args = process.argv.slice(2);
const noRedact = args.includes("--no-redact");
const mdOnly = args.includes("--md-only");
const outIdx = args.indexOf("--out");
const OUT_DIR = resolve(outIdx >= 0 ? args[outIdx + 1] : join(ROOT, "output", "report-sample"));

const t0 = Date.now();
const rawDir = join(OUT_DIR, "raw");
mkdirSync(rawDir, { recursive: true });

console.log(`[1/3] collect  → 并行采集 5 条只读命令…`);
const collected = await collect(ROOT);
for (const v of Object.values(collected.items)) {
  const mark = !v.ok ? "✗" : v.degraded ? "⚠" : "✓";
  const secs = `${((v.ms ?? 0) / 1000).toFixed(1)}s`;
  const tail = !v.ok ? ` — ${v.reason}` : v.degraded ? ` — 退出码 ${v.exitCode}（${v.exitNote}）` : "";
  console.log(`      ${mark} ${v.cmd} ${secs}${tail}`);
  if (v.ok) {
    writeFileSync(join(rawDir, `${v.key}.json`), JSON.stringify(v.data, null, 2));
    // raw/ 是本机原始数据（含真实路径），不是外发件
    writeFileSync(
      join(rawDir, "README.txt"),
      "本目录是报告生成时的原始 JSON（未脱敏，含真实路径）。\n外发只发上一级目录的 report.md / report.pdf。\n",
    );
  }
}

// 真实版本号（--version 失败则如实标注）
let pruneVersion = "该字段本版本不提供";
try {
  pruneVersion = execFileSync(collected.bin, ["--version"], { timeout: 10_000 }).toString().trim();
} catch {}

console.log(
  `[2/3] assemble → 组装九段 + 硬断言${noRedact ? "（脱敏关闭：仅限本机自用）" : "（脱敏开）"}…`,
);
const { md, ledger } = assemble(collected, { redact: !noRedact, pruneVersion });
writeFileSync(join(OUT_DIR, "report.md"), md);
console.log(`[3/3] render   → MD ✓（${ledger.sources.length} 条来源脚注）`);

const made = [join(OUT_DIR, "report.md")];
if (!mdOnly) {
  const { htmlPath, html } = render(md, OUT_DIR);
  writeFileSync(htmlPath, html);
  made.push(htmlPath);

  const pdfPath = join(OUT_DIR, "report.pdf");
  rmSync(pdfPath, { force: true }); // 先删旧件：失败时不能留上一份让人误发
  const pdf = printPdf(htmlPath, pdfPath);
  if (pdf.ok) {
    made.push(pdfPath);
    console.log(`               PDF ✓`);
  } else {
    console.log(`               PDF ✗ ${pdf.reason}`);
  }
}

const secs = ((Date.now() - t0) / 1000).toFixed(1);
const failures = Object.values(collected.items).filter((v) => !v.ok).length;
const degraded = Object.values(collected.items).filter((v) => v.ok && v.degraded).length;

console.log(`\n完成（${secs}s）→ ${OUT_DIR}`);
console.log(`  产出：${made.map((p) => p.split("/").pop()).join(" · ")}`);
if (failures || degraded) {
  console.log(`  注意：${failures} 条命令未取到数据、${degraded} 条以非 0 退出码返回（原因已写入报告）`);
}
console.log(`  raw/ 为本机原始 JSON（未脱敏），外发只发 report.md / report.pdf。`);

if (!existsSync(made[0])) process.exit(1);
