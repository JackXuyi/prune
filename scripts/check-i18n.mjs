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
import { fileURLToPath } from "node:url";

// 必须走 fileURLToPath，不能直接取 URL.pathname：Windows 上 pathname 是
// `/D:/a/prune/prune/`（带前导斜杠、正斜杠），join 之后变成 `\D:\a\...`，
// 被当成「当前盘的根目录下有个叫 D: 的文件夹」，于是 readFileSync 报
// `D:\D:\a\prune\prune\...` ENOENT —— 本地 macOS 永远复现不出来，
// 只有 Windows CI 打包时才炸（v0.7.0 首次发版的失败原因）。
const ROOT = fileURLToPath(new URL("..", import.meta.url));
const SRC = join(ROOT, "src");

/**
 * 仓库相对路径，**统一成正斜杠**。
 *
 * Windows 上 `relative()` 返回 `src\i18n\messages\chart.ts`，与 ALLOW 里的
 * `src/i18n/messages/` 前缀永远匹配不上 —— 结果是 catalog 本体 950 处中文
 * 全被当成违规。同一类 Windows 路径问题的第二个变体（第一个是 ROOT 用了
 * URL.pathname），两个都只在 Windows CI 上现形。
 */
const relOf = (abs) => relative(ROOT, abs).replaceAll("\\", "/");

/** 允许存在中文的路径前缀（相对仓库根，正斜杠） */
const ALLOW = [
  "src/i18n/messages/",
  // i18n 运行时本身：`localeName()` 返回「中文 / English」，这两个词在任何
  // 语言下都必须保持原文（用户否则看不出点哪个能换回自己看得懂的语言）
  "src/i18n/index.ts",
  "src/lib/devFixture.ts",
];

/**
 * 路径自检：拿一个确定存在于 ALLOW 里的文件验证整条链路。
 *
 * 没有这一步时，分隔符或基准目录一错，症状是「950 处硬编码中文」——
 * 看上去像文案出了问题，实际是路径匹配全挂。宁可一行报清楚原因。
 */
{
  const probe = relOf(join(SRC, "i18n", "index.ts"));
  if (!ALLOW.some((p) => probe === p || probe.startsWith(p))) {
    console.error(`[i18n] ❌ 路径自检失败：src/i18n/index.ts 解析成了 "${probe}"`);
    console.error("      ALLOW 前缀匹配不上，通常是 Windows 路径分隔符或 ROOT 基准不对");
    process.exit(1);
  }
}

/**
 * **没有待迁清单**（spec §8 commit 9）。
 *
 * 迁移期曾有一份 `PENDING` 数组按文件豁免；它已经清空并删除，因为留着一个
 * 「可以往里加文件」的入口，等于给下一句硬编码中文开了后门。现在 `src/**
 * 里除了 `ALLOW` 三个前缀，任何中文都会让构建失败。
 */

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
  const rel = relOf(abs);
  if (ALLOW.some((p) => rel === p || rel.startsWith(p))) continue;
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
