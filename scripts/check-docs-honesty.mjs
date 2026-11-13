#!/usr/bin/env node
/**
 * 文档诚信门禁（M1 / 2027 H1 Roadmap §9 M1 验收的两条自动化）：
 *
 *   检查 A —— MCP 工具对账：代码事实（prune-cli/src/mcp.rs 的 run_tool 分发臂
 *             与 tool_defs() 声明）必须与 README 声明的「N 个 / 工具清单」逐字一致。
 *             三处不一致即非零退出。防止 README 工具数与实现漂移（v0.8.4 时代
 *             5→8 的漂移教训：工具加了、文档没跟，读者按文档调工具直接 unknown tool）。
 *
 *   检查 B —— 排他表述 sweep：面向外部的文档（README / CHANGELOG / 两份 spec）
 *             禁止出现「独家 / 只有 Prune / 仍无竞品跟进 / 唯一把…」类排他性
 *             竞争表述（DP-4 裁 A：全删，改可核验事实陈述）。phys_footprint 口径
 *             已被 RAMWatcher 复制的教训：排他句式不可自证，写出去就是诚信欠账。
 *             语义性的「唯一」（唯一入口 / 唯一一次隐私弹窗 / 唯一破例）不匹配模式，
 *             不误伤；确需保留的表述可用 `<!-- 诚信检查豁免: 理由 -->` 行内标注。
 *
 * 接线：package.json build 链（check-i18n 之后）+ `pnpm check:docs` 本地单独跑。
 * 纯静态解析，零依赖、零构建，ubuntu runner 1 秒内完成。
 */

import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const problems = [];

// ---------------------------------------------------------------------------
// 检查 A：MCP 工具对账（代码 ↔ 代码 ↔ README，三方一致）
// ---------------------------------------------------------------------------

const mcpSource = readFileSync(join(root, 'crates/prune-cli/src/mcp.rs'), 'utf8');

// A1. run_tool 分发臂：真实工具的特征是臂体调用 tool_* 助手；
//     执行类动词臂映射 Err(...)（显式拒绝，M29），other 臂是兜底——都不算工具。
//     注意拒绝臂是多动词跨行写法（"reclaim" | "clean" | … | "trash" => Err(...)），
//     必须把同一臂内的全部动词都收进来——这条臂的存在本身就是门禁对象：
//     「执行类请求显式拒绝」若被误删，README 的「一扇执行的门都没有」就是假话。
const dispatchTools = [];
const rejectedVerbs = [];
{
  const startIdx = mcpSource.indexOf('pub fn run_tool(');
  const endIdx = mcpSource.indexOf('other =>', startIdx); // 兜底臂即函数收尾
  if (startIdx === -1 || endIdx === -1) {
    problems.push('mcp.rs 里找不到完整的 run_tool 函数（pub fn run_tool( / other => 兜底臂）');
  } else {
    const fnText = mcpSource.slice(startIdx, endIdx);
    // 臂形态："a" => / "a" | "b" | "c" =>（含跨行的管道连接）
    const armRe = /"([a-z_]+)"((?:\s*\|\s*"[a-z_]+")*)\s*=>/g;
    const matches = [...fnText.matchAll(armRe)];
    matches.forEach((m, i) => {
      const verbs = [m[1], ...(m[2].matchAll(/"([a-z_]+)"/g))].map((x) =>
        typeof x === 'string' ? x : x[1],
      );
      const bodyEnd = i + 1 < matches.length ? matches[i + 1].index : fnText.length;
      const body = fnText.slice(m.index + m[0].length, bodyEnd);
      if (body.includes('tool_')) {
        dispatchTools.push(...verbs);
      } else if (body.includes('Err(')) {
        rejectedVerbs.push(...verbs);
      } else {
        // 既不调工具、也不显式 Err 的臂 = 一条可执行的非工具通道——
        // 正是「一扇执行的门都没有」要防的东西，无论它长什么样都红。
        problems.push(
          `run_tool 出现非工具、非显式拒绝的可执行臂: ${verbs.join(', ')}（M29 只读底线回退）`,
        );
      }
    });
  }
}

// A2. tool_defs() 里声明的 name 集合（tools/list 暴露给 Agent 的清单）
const defsTools = [];
{
  const start = mcpSource.indexOf('fn tool_defs()');
  const end = mcpSource.indexOf('\n}', start); // 顶层闭合
  if (start === -1 || end === -1) {
    problems.push('mcp.rs 里找不到完整的 fn tool_defs() 函数体');
  } else {
    for (const m of mcpSource.slice(start, end).matchAll(/"name":\s*"([a-z_]+)"/g)) {
      defsTools.push(m[1]);
    }
  }
}

if (dispatchTools.length === 0) {
  problems.push('mcp.rs run_tool 提取到 0 个工具——解析失效，请检查脚本或源结构');
}
if (defsTools.length === 0) {
  problems.push('mcp.rs tool_defs() 提取到 0 个工具——解析失效，请检查脚本或源结构');
}

// A3. 声明必须与分发一致：缺 defs 的工具 = Agent 看得见名字调不通；缺 dispatch 的
//     = 反过来。两边都是诚信漏洞。
const onlyInDefs = defsTools.filter((t) => !dispatchTools.includes(t));
const onlyInDispatch = dispatchTools.filter((t) => !defsTools.includes(t));
if (onlyInDefs.length) {
  problems.push(`tool_defs() 声明了但 run_tool 未分发: ${onlyInDefs.join(', ')}`);
}
if (onlyInDispatch.length) {
  problems.push(`run_tool 可分发但 tool_defs() 未声明: ${onlyInDispatch.join(', ')}`);
}

// A3b. 只读底线断言：执行类动词的显式拒绝臂必须存在（M29）。README 宣称
//      「一扇执行的门都没有」，这条臂被删掉 = 叙事与代码脱钩，必须红。
if (rejectedVerbs.length === 0) {
  problems.push(
    'run_tool 里提取不到执行类动词的显式拒绝臂（Err(MCP 通道只读…)）——只读底线回退，回退即红',
  );
}

// A4. README 声明段对账
const readme = readFileSync(join(root, 'README.md'), 'utf8');
const readmeLines = readme.split('\n');
const headIdx = readmeLines.findIndex((l) => l.includes('MCP 工具（'));
if (headIdx === -1) {
  problems.push('README 里找不到「MCP 工具（N 个…」声明段');
} else {
  // 段落从声明头一直收到以「）。」结尾的行（工具清单跨多行）
  let para = '';
  for (let i = headIdx; i < readmeLines.length; i++) {
    para += readmeLines[i] + '\n';
    if (/）。\s*$/.test(readmeLines[i])) break;
  }
  const countM = para.match(/工具（(\d+) 个/);
  // 工具名在 README 里的形态是 `name`（描述）——反引号后紧跟全角括号；
  // 段内其他反引号 token（如 `coverage`）后面是普通文字，不会被捕获。
  const readmeTools = [...para.matchAll(/`([a-z_]+)`（/g)].map((m) => m[1]);

  if (countM) {
    const declared = Number(countM[1]);
    if (declared !== dispatchTools.length) {
      problems.push(
        `README 声明 MCP 工具 ${declared} 个，代码实际可分发 ${dispatchTools.length} 个` +
          `（${dispatchTools.join(', ')}）`,
      );
    }
  } else {
    problems.push('README「MCP 工具（N 个」的数字解析失败');
  }

  const missingInReadme = dispatchTools.filter((t) => !readmeTools.includes(t));
  const extraInReadme = readmeTools.filter((t) => !dispatchTools.includes(t));
  if (missingInReadme.length) {
    problems.push(`README MCP 清单缺工具: ${missingInReadme.join(', ')}`);
  }
  if (extraInReadme.length) {
    problems.push(`README MCP 清单多出（非工具）条目: ${extraInReadme.join(', ')}`);
  }
}

// ---------------------------------------------------------------------------
// 检查 B：排他表述 sweep（面向外部的文档）
// ---------------------------------------------------------------------------

const PUBLIC_DOCS = [
  'README.md',
  'CHANGELOG.md',
  'docs/prune-feature-spec-v2.md',
  'docs/prune-tauri-technical-plan.md',
];

// 排他性竞争表述模式（DP-4 裁 A）。语义性「唯一」（唯一入口 / 唯一一次 /
// 唯一破例）不在此列——只拦「拿竞品做参照系宣称自己独占」的句式。
const EXCLUSIVITY_PATTERNS = [
  /独家/,
  /只有 Prune/,
  /仍无竞品/,
  /没有竞品跟进/,
  /唯一把/,
  /没人占的位/,
  /Prune 是唯一/,
  /唯一.*无竞品/,
];

for (const rel of PUBLIC_DOCS) {
  let text;
  try {
    text = readFileSync(join(root, rel), 'utf8');
  } catch {
    problems.push(`诚信检查目标文件不存在: ${rel}（从 PUBLIC_DOCS 移除或补回文件）`);
    continue;
  }
  const lines = text.split('\n');
  lines.forEach((line, i) => {
    if (line.includes('诚信检查豁免')) return; // 显式标注的豁免行
    for (const p of EXCLUSIVITY_PATTERNS) {
      if (p.test(line)) {
        problems.push(
          `${rel}:${i + 1} 排他性表述（/${p.source}/）：${line.trim().slice(0, 80)}…` +
            `\n        改法：改为可核验的事实陈述，或行内加 <!-- 诚信检查豁免: 理由 -->`,
        );
        break; // 每行只报第一条命中
      }
    }
  });
}

// ---------------------------------------------------------------------------
// 汇总输出
// ---------------------------------------------------------------------------

if (problems.length) {
  console.error(`✗ 文档诚信检查失败（${problems.length} 项）：\n`);
  for (const p of problems) console.error('  - ' + p);
  console.error(
    `\n  实际可分发 MCP 工具（${dispatchTools.length}）：${dispatchTools.join(', ')}` +
      `\n  显式拒绝动词（${rejectedVerbs.length}）：${rejectedVerbs.join(', ')}`,
  );
  process.exit(1);
}

console.log(
  `✓ 文档诚信检查通过：MCP ${dispatchTools.length} 个工具三处对账一致` +
    `（dispatch ↔ tool_defs ↔ README）；排他表述 sweep ${PUBLIC_DOCS.length} 份公开文档零命中` +
    `；执行类动词显式拒绝 ${rejectedVerbs.length} 个（${rejectedVerbs.join(', ')}）`,
);
