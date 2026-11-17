// W1-2 体检样例报告 · Step 3 render
// MD → HTML（A4 打印样式）→ PDF（系统 Chrome headless，零新依赖，DP-R3）。
// 检测不到 Chrome 时只出 MD 并提示（RK-R1 fallback）。

import { execFileSync } from "node:child_process";
import { existsSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const CHROME_CANDIDATES = [
  "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
  "/Applications/Chromium.app/Contents/MacOS/Chromium",
  "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
  "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser",
  "/usr/bin/google-chrome",
  "/usr/bin/chromium",
];

function findChrome() {
  for (const c of CHROME_CANDIDATES) if (existsSync(c)) return c;
  return null;
}

// ---------------------------------------------------------------- MD → HTML
// 只支持报告模板实际用到的语法：标题 / 段落 / 无序列表（两级）/ 表格 / 引用 / 分隔线。
// 手写而非引库：repo 零模板引擎依赖，为一个脚本破例不划算（方案 §3.2）。

const esc = (s) => s.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;");

function inline(s) {
  return esc(s)
    // 脚注 → 可点击上标，指向来源附录的对应行
    .replaceAll(/\[\^s(\d+)\]/g, '<a class="src" href="#src-s$1">S$1</a>')
    .replaceAll(/\*\*(.+?)\*\*/g, "<b>$1</b>")
    .replaceAll(/`(.+?)`/g, "<code>$1</code>");
}

function renderCells(line) {
  return line
    .replace(/^\|/, "")
    .replace(/\|$/, "")
    .split("|")
    .map((c) => c.trim());
}

function renderTable(rows) {
  const [head, ...body] = rows;
  const th = head.map((c) => `<th>${inline(c)}</th>`).join("");
  const tr = body
    .map(
      (r) =>
        `<tr>${r
          .map((c) => {
            const m = c.match(/^S(\d+)$/);
            return `<td>${m ? `<span class="src-id" id="src-s${m[1]}">S${m[1]}</span>` : inline(c)}</td>`;
          })
          .join("")}</tr>`,
    )
    .join("\n");
  return `<table><thead><tr>${th}</tr></thead><tbody>${tr}</tbody></table>`;
}

function renderList(items, start, indent) {
  let out = "<ul>";
  let i = start;
  while (i < items.length && items[i].indent >= indent) {
    const cur = items[i];
    if (cur.indent > indent) {
      const [sub, ni] = renderList(items, i, cur.indent);
      out = out.replace(/<\/li>$/, `${sub}</li>`); // 挂到上一个条目下
      i = ni;
      continue;
    }
    if (i + 1 < items.length && items[i + 1].indent > indent) {
      const [sub, ni] = renderList(items, i + 1, items[i + 1].indent);
      out += `<li>${inline(cur.text)}${sub}</li>`;
      i = ni;
    } else {
      out += `<li>${inline(cur.text)}</li>`;
      i++;
    }
  }
  return [out + "</ul>", i];
}

const isTableRow = (l) => /^\s*\|/.test(l);
const isTableSep = (l) => /^\s*\|[\s:|-]+\|?\s*$/.test(l) && l.includes("-");
const isListItem = (l) => /^(\s*)[-*] /.test(l);
const isHeading = (l) => /^#{1,4} /.test(l);
const isHr = (l) => /^---\s*$/.test(l);
const isQuote = (l) => /^> /.test(l);

function mdToHtml(md) {
  const lines = md.split("\n");
  const out = [];
  let i = 0;

  while (i < lines.length) {
    const line = lines[i];

    if (!line.trim()) {
      i++;
      continue;
    }
    if (isHr(line)) {
      out.push("<hr/>");
      i++;
      continue;
    }
    if (isHeading(line)) {
      const m = line.match(/^(#{1,4}) (.*)$/);
      out.push(`<h${m[1].length}>${inline(m[2])}</h${m[1].length}>`);
      i++;
      continue;
    }
    if (isQuote(line)) {
      const buf = [];
      while (i < lines.length && isQuote(lines[i])) buf.push(lines[i++].replace(/^> ?/, ""));
      out.push(`<blockquote>${inline(buf.join(" "))}</blockquote>`);
      continue;
    }
    if (isTableRow(line)) {
      const rows = [];
      while (i < lines.length && isTableRow(lines[i])) {
        if (!isTableSep(lines[i])) rows.push(renderCells(lines[i]));
        i++;
      }
      if (rows.length) out.push(renderTable(rows));
      continue;
    }
    if (isListItem(line)) {
      const items = [];
      while (i < lines.length && isListItem(lines[i])) {
        const m = lines[i].match(/^(\s*)[-*] (.*)$/);
        items.push({ indent: Math.floor(m[1].length / 2), text: m[2] });
        i++;
      }
      const [html] = renderList(items, 0, items[0].indent);
      out.push(html);
      continue;
    }

    // 普通段落：吃到空行或另一个块的起点
    const buf = [];
    while (
      i < lines.length &&
      lines[i].trim() &&
      !isHr(lines[i]) &&
      !isHeading(lines[i]) &&
      !isTableRow(lines[i]) &&
      !isListItem(lines[i]) &&
      !isQuote(lines[i])
    ) {
      buf.push(lines[i++]);
    }
    out.push(`<p>${inline(buf.join(" "))}</p>`);
  }

  return `<!doctype html><html lang="zh-CN"><head><meta charset="utf-8">
<title>开发机体检报告</title><style>
@page { size: A4; margin: 16mm 15mm; }
* { box-sizing: border-box; }
/* 显式白底：新 headless 默认打印背景，body 不设底色会得到透明页，
   在深色查看器里会合成成黑底黑字 */
html, body { background: #fff; -webkit-print-color-adjust: exact; print-color-adjust: exact; }
body { font: 10.5pt/1.6 "PingFang SC", "Hiragino Sans GB", "Microsoft YaHei", sans-serif;
       color: #1a1a1a; margin: 0; }
h1 { font-size: 19pt; margin: 0 0 4pt; letter-spacing: .5pt; }
h2 { font-size: 13pt; margin: 14pt 0 5pt; padding-bottom: 3pt;
     border-bottom: 1.2pt solid #333; break-after: avoid; }
h3, h4 { font-size: 11.5pt; margin: 9pt 0 3pt; break-after: avoid; }
p { margin: 3pt 0; }
ul { margin: 3pt 0; padding-left: 15pt; }
ul ul { margin: 1pt 0; padding-left: 13pt; }
li { margin: 2pt 0; }
blockquote { margin: 6pt 0; padding: 4pt 10pt; border-left: 2.5pt solid #999;
             color: #444; background: #fafafa; }
table { border-collapse: collapse; width: 100%; margin: 5pt 0; break-inside: avoid; }
th, td { border: 0.5pt solid #bbb; padding: 2.5pt 5pt; text-align: left; vertical-align: top;
         font-variant-numeric: tabular-nums; }
th { background: #f0f0f0; font-weight: 600; }
tr { break-inside: avoid; }
code { font: 8.8pt "SF Mono", Menlo, monospace; background: #f4f4f4;
       padding: 0 2.5pt; border-radius: 2pt; }
a.src { font-size: 6.8pt; color: #06c; text-decoration: none; vertical-align: super;
        margin-left: 1pt; }
span.src-id { font: 8.5pt "SF Mono", Menlo, monospace; color: #555; }
hr { border: none; border-top: 0.5pt solid #ccc; margin: 10pt 0; }
</style></head><body>
${out.join("\n")}
</body></html>`;
}

export function render(md, outDir) {
  const html = mdToHtml(md);
  return { htmlPath: join(outDir, "report.html"), html };
}

// ---------------------------------------------------------------- HTML → PDF

/**
 * Chrome headless 打 PDF。
 * 必须带独立 --user-data-dir：用户已开着 Chrome 时，共用默认 profile 会直接崩，
 * 且失败是静默的（只写 stderr），此前因此产出过"上一次留下的旧 PDF"。
 */
export function printPdf(htmlPath, pdfPath) {
  const chrome = findChrome();
  if (!chrome) {
    return {
      ok: false,
      reason: "未找到系统 Chrome/Edge/Chromium，只交付 MD/HTML（可用浏览器打开 report.html 手动打印）",
    };
  }
  if (existsSync(pdfPath)) rmSync(pdfPath, { force: true }); // 失败时绝不能留下旧文件

  const profile = join(tmpdir(), `prune-report-${process.pid}`);
  const args = [
    "--headless=new",
    "--disable-gpu",
    "--no-first-run",
    "--no-default-browser-check",
    "--disable-extensions",
    "--disable-background-networking",
    `--user-data-dir=${profile}`,
    "--no-pdf-header-footer",
    `--print-to-pdf=${pdfPath}`,
    `file://${htmlPath}`,
  ];

  let lastErr = null;
  for (let attempt = 0; attempt < 2; attempt++) {
    try {
      execFileSync(chrome, args, { timeout: 90_000, stdio: ["ignore", "ignore", "pipe"] });
      if (!existsSync(pdfPath)) throw new Error("Chrome 返回成功但未写出 PDF");
      rmSync(profile, { recursive: true, force: true });
      return { ok: true };
    } catch (e) {
      lastErr = e;
      const detail = (e.stderr ?? "").toString().split("\n").filter(Boolean).slice(-2).join(" / ");
      lastErr = new Error(`${e.message.split("\n")[0]}${detail ? `（${detail.slice(0, 160)}）` : ""}`);
    }
  }
  rmSync(profile, { recursive: true, force: true });
  return { ok: false, reason: `Chrome headless 失败: ${lastErr.message}（MD/HTML 已生成）` };
}
