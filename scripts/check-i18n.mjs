#!/usr/bin/env node
/**
 * 文案门禁（docs/i18n-spec.md §7.3）。
 *
 * 规则：`src/**` 里除注释外**不得出现任何中文字符** —— 中文只允许活在两处：
 * `src/i18n/messages/**`（catalog 本体）和 `src/lib/devFixture.ts`（dev 假数据）。
 *
 * 为什么不用正则扫字符串字面量：JSX 文本节点（`<span>尚未扫描</span>`）占了
 * UI 文案的大头，而它根本不在引号里。所以这里走一遍极简分词器：区分字符串 /
 * 模板串 / 行注释 / 块注释 / JSX 注释，只在「代码」状态下收集中文。
 *
 * 另外两条附加检查（都是踩过的坑）：
 * - `src/lib/**` 出现 `tGlobal(` → 纯函数读全局，切语言后旧串不更新
 * - 出现 `toLocaleTimeString("zh-CN")` 之类的硬编码 locale → 英文用户看到中文格式
 *
 * 用法：`node scripts/check-i18n.mjs`，退出码 0 = 通过。
 */
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";

const ROOT = new URL("..", import.meta.url).pathname;
const SRC = join(ROOT, "src");

/** 允许存在中文的路径前缀（相对仓库根） */
const ALLOW = [
  "src/i18n/messages/",
  // i18n 运行时本身：`localeName()` 返回「中文 / English」，这两个词在任何
  // 语言下都必须保持原文（用户否则看不出点哪个能换回自己看得懂的语言）
  "src/i18n/index.ts",
  "src/lib/devFixture.ts",
];

/**
 * 尚未迁移的文件：每完成一批就从这里删掉，最终必须清空（spec §8 commit 9）。
 *
 * 这个清单不是「白名单」，是**待办**：它只会变短。想加文件进来先问自己
 * 「这真是新的待迁文件，还是我又写了一句中文」。
 */
const PENDING = [
  "src/components/Sparkline.tsx",
  "src/components/charts/BarSeries.tsx",
  "src/components/charts/MeterBar.tsx",
  "src/components/charts/StackBar.tsx",
  "src/lib/ipc.ts",
  "src/lib/uiText.ts",
  "src/panels/Island.tsx",
  "src/panels/MenuBarPanel.tsx",
  "src/stores/diskStore.ts",
  "src/stores/memStore.ts",
  "src/stores/netTotalStore.ts",
  "src/stores/portStore.ts",
  "src/types/appdisk.ts",
];

const CJK = /[\u4e00-\u9fff]/;

function walk(dir, out = []) {
  for (const name of readdirSync(dir)) {
    if (name.startsWith(".")) continue;
    const p = join(dir, name);
    if (statSync(p).isDirectory()) walk(p, out);
    else if (/\.(ts|tsx)$/.test(name)) out.push(p);
  }
  return out;
}

/**
 * 剥注释 + 收集「代码区」的中文。
 * 返回 [{ line, text }]
 */
function scan(code) {
  const hits = [];
  let line = 1;
  let i = 0;
  const n = code.length;
  // 状态机：code / line-comment / block-comment / single / double / template
  let state = "code";

  const push = (startLine) => {
    // 取该行原文，便于定位（只截 100 字符）
    const lines = code.split("\n");
    const raw = (lines[startLine - 1] ?? "").trim().slice(0, 100);
    hits.push({ line: startLine, text: raw });
  };

  while (i < n) {
    const c = code[i];
    const next = code[i + 1];

    if (c === "\n") {
      line += 1;
      if (state === "line-comment") state = "code";
      i += 1;
      continue;
    }

    switch (state) {
      case "code":
        if (c === "/" && next === "/") {
          state = "line-comment";
          i += 2;
          continue;
        }
        if (c === "/" && next === "*") {
          state = "block-comment";
          i += 2;
          continue;
        }
        if (c === '"') {
          state = "double";
          i += 1;
          continue;
        }
        if (c === "'") {
          state = "single";
          i += 1;
          continue;
        }
        if (c === "`") {
          state = "template";
          i += 1;
          continue;
        }
        if (CJK.test(c)) push(line);
        i += 1;
        break;
      case "line-comment":
        i += 1;
        break;
      case "block-comment":
        if (c === "*" && next === "/") {
          state = "code";
          i += 2;
        } else i += 1;
        break;
      case "double":
        // 字符串里的中文 = 硬编码文案，同样违规
        if (CJK.test(c)) push(line);
        if (c === "\\") i += 2;
        else {
          if (c === '"') state = "code";
          i += 1;
        }
        break;
      case "single":
        if (CJK.test(c)) push(line);
        if (c === "\\") i += 2;
        else {
          if (c === "'") state = "code";
          i += 1;
        }
        break;
      case "template":
        if (CJK.test(c)) push(line);
        if (c === "\\") i += 2;
        else {
          if (c === "`") state = "code";
          i += 1;
        }
        break;
      default:
        i += 1;
    }
  }
  // 同一行重复命中只报一次
  const seen = new Set();
  return hits.filter((h) => (seen.has(h.line) ? false : (seen.add(h.line), true)));
}

const violations = [];
const extra = [];
const missingCache = [];

/**
 * 跨语言闸门：`whitelist.rs` 里每一条 `key` 都必须在 catalog 里有
 * `disk.cache.<key>`。缓存名的 key 集合由 Rust 决定，前端枚举不完 ——
 * 只能在构建时对着源码核对，否则发版当天才发现英文界面里冒出中文缓存名。
 */
const WHITELIST = join(ROOT, "crates/prune-core/src/disk/whitelist.rs");
const DISK_CATALOG = join(ROOT, "src/i18n/messages/namespaces/disk.ts");
{
  const rs = readFileSync(WHITELIST, "utf8");
  const ts = readFileSync(DISK_CATALOG, "utf8");
  // 只看 ENTRIES 里的 key（label/rel 也是字符串，不能整文件乱扫）
  const keys = [...rs.matchAll(/^\s*key:\s*"([^"]+)"/gm)].map((m) => m[1]);
  // 只核中文块：英文侧由 `MessageTree<typeof zh>` 在 tsc 里强制补齐，
  // 两套都扫一遍没意义，但扫错块（扫到英文）会让「中文漏 key」漏网。
  const zhOnly = ts.slice(0, ts.indexOf("const en: MessageTree"));
  const cacheBlock = zhOnly.slice(zhOnly.indexOf("cache: {"));
  for (const k of keys) {
    const bare = /^[a-z][a-z0-9]*$/i.test(k);
    const pattern = bare ? new RegExp(`^\\s*${k}:`, "m") : new RegExp(`^\\s*"${k}":`, "m");
    if (!pattern.test(cacheBlock)) missingCache.push(k);
  }
}

for (const abs of walk(SRC)) {
  const rel = relative(ROOT, abs);
  if (ALLOW.some((p) => rel === p || rel.startsWith(p))) continue;
  if (PENDING.includes(rel)) continue;

  const code = readFileSync(abs, "utf8");
  for (const h of scan(code)) violations.push(`${rel}:${h.line}  ${h.text}`);

  if (rel.startsWith("src/lib/") && /\btGlobal\s*\(/.test(code)) {
    extra.push(`${rel}  纯函数里用了 tGlobal（改成传 locale 参数）`);
  }
  const badLocale = code.match(/toLocale\w*\(\s*"zh-CN"/);
  if (badLocale) extra.push(`${rel}  硬编码 zh-CN 时间/数字格式`);
}

if (violations.length === 0 && extra.length === 0 && missingCache.length === 0) {
  console.log("[i18n] ✅ 无硬编码中文");
  process.exit(0);
}

if (missingCache.length) {
  console.error(`[i18n] ❌ ${missingCache.length} 条后端缓存名缺 catalog 条目：`);
  for (const k of missingCache) console.error(`  disk.cache.${k}  （whitelist.rs）`);
}
if (violations.length) {
  console.error(`[i18n] ❌ ${violations.length} 处硬编码中文：`);
  for (const v of violations) console.error("  " + v);
}
if (extra.length) {
  console.error(`[i18n] ❌ ${extra.length} 处规则违规：`);
  for (const v of extra) console.error("  " + v);
}
process.exit(1);
