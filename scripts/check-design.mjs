#!/usr/bin/env node
/**
 * 设计门禁（docs/ui-eul-design-language.md 的自动化子集）。
 *
 * 只把「能静态判定、且真的踩过坑」的规则做成阻断，其余靠文档 + DoD 清单。
 * 宁可少两条规则，也不要造出一堆开发者天天绕过的噪音。
 *
 *   R1 —— 图标方向 hack：`rotate-180` 拼上传图标（§7.3 事故）
 *        方向必须由成对图标定义（UploadIcon / DownloadIcon），不靠 transform 翻转。
 *
 *   R2 —— 主题令牌色硬编码：tsx 里出现 `--color-*` 令牌的十六进制字面值（§1.2）
 *        颜色一律走 `var(--color-*)` 或 Tailwind 语义类，否则浅色主题翻不过来。
 *        灵动岛（src/panels/Island.tsx）是固定深玻璃材质、不随主题翻转，豁免。
 *
 * 另附一条**不阻断**的提示：`font-mono` 未配 `tabular-nums` 的行数（§4.3）。
 * 它只报数量不报路径——数字对齐缺失值得知道，但不该拦住构建。
 *
 * 用法：`node scripts/check-design.mjs`，退出码 0 = 通过。
 */
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = fileURLToPath(new URL("..", import.meta.url));
const SRC = join(ROOT, "src");
const CSS = join(SRC, "index.css");

/** 豁免文件（相对仓库根、正斜杠）。加条目必须写清理由。 */
const ALLOW = new Map([
  ["src/panels/Island.tsx", "灵动岛固定深玻璃材质，白色系与 iOS 系统色不随主题翻转"],
]);

const problems = [];

/** 仓库相对路径，统一正斜杠（Windows 上 relative() 给反斜杠，前缀匹配会失效） */
const relOf = (abs) => relative(ROOT, abs).replaceAll("\\", "/");

function walk(dir, out = []) {
  for (const name of readdirSync(dir)) {
    const abs = join(dir, name);
    if (statSync(abs).isDirectory()) walk(abs, out);
    else if (/\.(ts|tsx)$/.test(name)) out.push(abs);
  }
  return out;
}

/**
 * 去掉注释、保留行号：块注释跨行，用字符级状态机比逐行正则可靠。
 * 返回 [{ line, text }]，text 为剥离注释后的代码。
 */
function stripComments(src) {
  const out = [];
  let line = 1;
  let buf = "";
  let i = 0;
  let inBlock = false;
  let inLine = false;
  let inStr = null; // 当前字符串引号（' " `）

  while (i < src.length) {
    const ch = src[i];
    const next = src[i + 1];

    if (inLine) {
      if (ch === "\n") {
        inLine = false;
        out.push({ line, text: buf });
        buf = "";
        line += 1;
      }
      i += 1;
      continue;
    }
    if (inBlock) {
      if (ch === "*" && next === "/") {
        inBlock = false;
        i += 2;
        continue;
      }
      if (ch === "\n") {
        out.push({ line, text: buf });
        buf = "";
        line += 1;
      }
      i += 1;
      continue;
    }
    if (inStr) {
      // 字符串里的 // 不是注释；不过转义与模板串插值这里不做完整解析，
      // 门禁只关心类名/色值，够用即可。
      if (ch === "\\") {
        buf += ch + (next ?? "");
        i += 2;
        continue;
      }
      if (ch === inStr) inStr = null;
      buf += ch;
      i += 1;
      continue;
    }

    if (ch === "/" && next === "*") {
      inBlock = true;
      i += 2;
      continue;
    }
    if (ch === "/" && next === "/") {
      inLine = true;
      i += 2;
      continue;
    }
    if (ch === '"' || ch === "'" || ch === "`") {
      inStr = ch;
      buf += ch;
      i += 1;
      continue;
    }
    if (ch === "\n") {
      out.push({ line, text: buf });
      buf = "";
      line += 1;
      i += 1;
      continue;
    }
    buf += ch;
    i += 1;
  }
  out.push({ line, text: buf });
  return out;
}

/** 从 index.css 的 @theme 抽 `--color-*: #rrggbb`（浅色覆盖也是同名，取并集） */
function tokenColors() {
  const css = readFileSync(CSS, "utf8");
  const map = new Map();
  for (const m of css.matchAll(/(--color-[a-z0-9-]+)\s*:\s*(#[0-9a-fA-F]{6})\b/g)) {
    const hex = m[2].toLowerCase();
    if (!map.has(hex)) map.set(hex, m[1]);
  }
  return map;
}

const tokens = tokenColors();
const files = walk(SRC);
let monoNoTabular = 0;

for (const abs of files) {
  const rel = relOf(abs);
  const lines = stripComments(readFileSync(abs, "utf8"));

  for (const { line, text } of lines) {
    // R1：图标方向 hack
    if (/rotate-180/.test(text)) {
      problems.push({
        rule: "R1",
        rel,
        line,
        msg: "图标不得用 rotate-180 翻转凑方向——用成对图标（UploadIcon / DownloadIcon）",
        snippet: text.trim().slice(0, 100),
      });
    }

    // R2：主题令牌色硬编码
    if (!ALLOW.has(rel)) {
      for (const m of text.matchAll(/#[0-9a-fA-F]{6}\b/g)) {
        const token = tokens.get(m[0].toLowerCase());
        if (token) {
          problems.push({
            rule: "R2",
            rel,
            line,
            msg: `硬编码主题色 ${m[0]}（即 ${token}）——改用 var(${token}) 或 Tailwind 语义类`,
            snippet: text.trim().slice(0, 100),
          });
        }
      }
    }

    // 提示项：mono 数字没对齐
    if (/font-mono/.test(text) && !/tabular-nums/.test(text)) monoNoTabular += 1;
  }
}

if (problems.length > 0) {
  for (const p of problems) {
    console.error(`${p.rel}:${p.line}  [${p.rule}] ${p.msg}\n    ${p.snippet}`);
  }
  console.error(
    `\n设计门禁失败：${problems.length} 处。规则见 docs/ui-eul-design-language.md §1.2 / §7.3。`,
  );
  process.exit(1);
}

const waived = [...ALLOW.keys()].length;
console.log(
  `设计门禁通过（${files.length} 个文件，${tokens.size} 个主题色令牌，豁免 ${waived} 个材质文件）` +
    (monoNoTabular > 0 ? `；提示：${monoNoTabular} 处 font-mono 未配 tabular-nums` : ""),
);
