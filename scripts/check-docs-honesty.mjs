#!/usr/bin/env node
/**
 * 文档诚信门禁（M1 / 2027 H1 Roadmap §9 M1 验收的两条自动化）：
 *
 *   检查 A —— MCP 工具对账：代码事实（prune-cli/src/mcp.rs 的 run_tool 分发臂
 *             与 tool_defs() 声明）必须与 README 声明的「N 个 / 工具清单」逐字一致。
 *             **中英双版各查一遍**（README.md 英文默认版 / README.zh-CN.md 中文版），
 *             缺一份即红——只查一份的话，另一份漂移就没人拦。
 *             三处不一致即非零退出。防止 README 工具数与实现漂移（v0.8.4 时代
 *             5→8 的漂移教训：工具加了、文档没跟，读者按文档调工具直接 unknown tool）。
 *
 *   检查 B —— 排他表述 sweep：面向外部的文档（README 中英双版 / CHANGELOG /
 *             两份 spec）禁止出现「独家 / 只有 Prune / 仍无竞品跟进 / 唯一把…」类
 *             排他性竞争表述（DP-4 裁 A：全删，改可核验事实陈述）。phys_footprint
 *             口径已被 RAMWatcher 复制的教训：排他句式不可自证，写出去就是诚信欠账。
 *             语义性的「唯一」（唯一入口 / 唯一一次隐私弹窗 / 唯一破例）不匹配模式，
 *             不误伤；确需保留的表述可用 `<!-- 诚信检查豁免: 理由 -->` 行内标注。
 *
 *   检查 C —— 编辑器配方对账（W2/D-12）：`docs/editors/*.md` 里出现的每条
 *             `prune ...` 命令，子命令与 flag 必须能在 `main.rs` 的 usage()
 *             字符串里找到——配方是给用户逐字粘贴的，命令漂移 = 用户照文档
 *             敲出「不认识子命令」。同时要求三份配方各自覆盖 port / projects /
 *             watch 三个子命令（方案 §9.2 的任务集不许被静默删减）。
 *
 *   检查 D —— cli-reference 对账（W1/D-06）：`docs/cli-reference.md` 的每个
 *             `prune <子命令>`（章节标题 + shell 行 + 反引号命令）必须在 usage()
 *             里；usage() 的每个子命令必须有对应小节；文档里的 flag ⊆ usage
 *             flags。usage 是命令面的唯一权威，双向漂移即红。
 *
 *   检查 E —— 退出码对账（W1/D-06）：`cli-reference.md` 退出码表 ↔ main.rs
 *             `ExitCode` 枚举 ↔ usage() 退出码行，三方集合双向相等——
 *             防止再出现「usage 写了 3、代码从不返回」类脱节（W1-DP-2 修复
 *             的教训固化成门禁）。
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

// A4. README 声明段对账（中英双版：README.md 默认英文，README.zh-CN.md 中文）
//
//     两份都要查——只查一份的话，另一份漂移就没人拦，而读者看哪一份都可能。
//     声明头两种写法：英文 `**MCP tools (8, all read-only):**` / 中文
//     `**MCP 工具（8 个，全部只读）：**`；工具名两种写法：`name`（描述）/
//     `name` (description)。段落边界取「声明头 → 第一个空行」，工具清单跨行时
//     不依赖句末标点（中英文句号不同，靠标点断句太脆）。
const README_FILES = ['README.md', 'README.zh-CN.md'];
const DECL_RE = /(?:工具[（(]|tools?\s*[（(])\s*(\d+)/;

for (const rel of README_FILES) {
  let text;
  try {
    text = readFileSync(join(root, rel), 'utf8');
  } catch {
    problems.push(`README 对账目标文件不存在: ${rel}（双语文档必须成对存在）`);
    continue;
  }
  const lines = text.split('\n');
  const headIdx = lines.findIndex((l) => /MCP/.test(l) && DECL_RE.test(l));
  if (headIdx === -1) {
    problems.push(`${rel} 里找不到「MCP tools (N…」/「MCP 工具（N 个…」声明段`);
    continue;
  }

  let para = '';
  for (let i = headIdx; i < lines.length; i++) {
    if (i > headIdx && lines[i].trim() === '') break;
    para += lines[i] + '\n';
  }
  const countM = para.match(DECL_RE);
  // 工具名在 README 里的形态是 `name`（描述）——反引号后紧跟括号（中英双版）；
  // 段内其他反引号 token（如 `coverage`）后面是普通文字，不会被捕获。
  const readmeTools = [...para.matchAll(/`([a-z_]+)`\s*[（(]/g)].map((m) => m[1]);

  if (countM) {
    const declared = Number(countM[1]);
    if (declared !== dispatchTools.length) {
      problems.push(
        `${rel} 声明 MCP 工具 ${declared} 个，代码实际可分发 ${dispatchTools.length} 个` +
          `（${dispatchTools.join(', ')}）`,
      );
    }
  } else {
    problems.push(`${rel}「MCP tools (N」/「MCP 工具（N 个」的数字解析失败`);
  }

  const missingInReadme = dispatchTools.filter((t) => !readmeTools.includes(t));
  const extraInReadme = readmeTools.filter((t) => !dispatchTools.includes(t));
  if (missingInReadme.length) {
    problems.push(`${rel} MCP 清单缺工具: ${missingInReadme.join(', ')}`);
  }
  if (extraInReadme.length) {
    problems.push(`${rel} MCP 清单多出（非工具）条目: ${extraInReadme.join(', ')}`);
  }
}

// ---------------------------------------------------------------------------
// 检查 B：排他表述 sweep（面向外部的文档）
// ---------------------------------------------------------------------------

const PUBLIC_DOCS = [
  'README.md',
  'README.zh-CN.md',
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
// 检查 C：编辑器配方命令对账（docs/editors/*.md ↔ main.rs usage()）
// ---------------------------------------------------------------------------

const EDITOR_DOCS = ['docs/editors/vscode.md', 'docs/editors/jetbrains.md', 'docs/editors/zed.md'];

// C1. 从 main.rs 提取 usage() 字符串——它是命令面的唯一权威
const cliSource = readFileSync(join(root, 'crates/prune-cli/src/main.rs'), 'utf8');
const usageStart = cliSource.indexOf('fn usage() -> String {');
const usageEnd = cliSource.indexOf('fn main()', usageStart);
if (usageStart === -1 || usageEnd === -1) {
  problems.push('main.rs 里找不到 usage() 函数体（检查 C 解析失效）');
}
const usageText =
  usageStart !== -1 && usageEnd !== -1 ? cliSource.slice(usageStart, usageEnd) : '';

// 子命令集合：usage 文本里 "  prune <子命令>" 行首的词
const USAGE_SUBCOMMANDS = [...usageText.matchAll(/^\s+prune ([a-z]+)/gm)].map((m) => m[1]);
// flag 白名单：usage 文本里出现过的 --xxx / -x 形态 token。
// usage 里 flag 常写成 [--json] / [--all | -a]——前面的分隔符是 `[` 或 `|`，
// 所以分隔符集要含 [\s(|。
const usageFlags = new Set(
  [...usageText.matchAll(/(?:^|[\s(|[])(--[a-z][a-z-]*|-[a-z])\b/g)].map((m) => m[1]),
);

if (usageText && USAGE_SUBCOMMANDS.length === 0) {
  problems.push('main.rs usage() 提取到 0 个子命令——解析失效，请检查脚本或源结构');
}

// C2. 从配方文档提取「会被用户逐字粘贴/填写」的命令字符串：
//     - JSON 任务片段：`"command": "prune …"`
//     - JetBrains 表格：`| Arguments | `…` |`（Program 行只有 `prune`，无子命令，跳过）
//     - shell 块里的直跑行：以 `prune ` 开头的行
const editorCommands = [];
for (const rel of EDITOR_DOCS) {
  let text;
  try {
    text = readFileSync(join(root, rel), 'utf8');
  } catch {
    problems.push(`编辑器配方文件不存在: ${rel}（方案 §9.2 要求三份，删文件必须先改方案）`);
    continue;
  }

  const found = [];
  for (const m of text.matchAll(/"command"\s*:\s*"([^"]+)"/g)) found.push(m[1]);
  for (const m of text.matchAll(/^\s*\|\s*(?:Arguments|Command)\s*\|\s*`([^`]+)`\s*\|/gm)) {
    found.push(m[1]);
  }
  for (const line of text.split('\n')) {
    const s = line.trim();
    if (s.startsWith('prune ') && !s.includes('```')) found.push(s);
  }

  const covered = new Set();
  for (const raw of found) {
    let tokens = raw.trim().split(/\s+/);
    // JetBrains 表格的 Arguments 行不带 "prune" 前缀（Program 行单独写）——
    // 仅当首 token 本身是已知子命令时才补前缀，散文/示例值静默跳过
    if (tokens[0] !== 'prune' && USAGE_SUBCOMMANDS.includes(tokens[0])) {
      tokens = ['prune', ...tokens];
    }
    if (tokens[0] !== 'prune' || tokens.length < 2) continue; // "prune" 单独出现（Program 行）或非命令
    const sub = tokens[1];
    covered.add(sub);
    // 子命令校验：裸 flag 子命令（--version/--help）按 usage 文本子串校验
    if (sub.startsWith('-')) {
      if (usageText && !usageText.includes(sub)) {
        problems.push(`${rel} 配方用了 usage 里不存在的子命令: ${raw}`);
      }
      continue;
    }
    if (!USAGE_SUBCOMMANDS.includes(sub)) {
      problems.push(
        `${rel} 配方子命令 "${sub}" 不在 main.rs usage() 里（实际子命令: ${USAGE_SUBCOMMANDS.join(', ')}）: ${raw}`,
      );
      continue;
    }
    // flag 校验：每个 - 开头的 token 必须在 usage 文本里出现过
    for (const tk of tokens.slice(2)) {
      if (tk.startsWith('-') && !usageFlags.has(tk)) {
        problems.push(`${rel} 配方 flag "${tk}" 不在 main.rs usage() 里: ${raw}`);
      }
    }
  }

  // C3. 任务集不许被静默删减：三份配方各自覆盖 port / projects / watch
  for (const required of ['port', 'projects', 'watch']) {
    if (!covered.has(required)) {
      problems.push(`${rel} 缺子命令 "${required}" 的配方（方案 §9.2 任务集要求覆盖）`);
    }
  }
}

// ---------------------------------------------------------------------------
// 检查 D：cli-reference 对账（docs/cli-reference.md ↔ main.rs usage()，双向）
// ---------------------------------------------------------------------------

const CLI_REF = 'docs/cli-reference.md';
let refText = '';
try {
  refText = readFileSync(join(root, CLI_REF), 'utf8');
} catch {
  problems.push(`${CLI_REF} 不存在（W1/D-06 要求 CLI 唯一参考文档）`);
}

if (refText) {
  // D1. 章节覆盖：usage 的每个子命令必须有 `## prune <sub>` 小节
  const refSections = new Set([...refText.matchAll(/^## prune ([a-z-]+)/gm)].map((m) => m[1]));
  const usageSubSet = new Set(USAGE_SUBCOMMANDS);
  for (const sub of usageSubSet) {
    if (!refSections.has(sub)) {
      problems.push(`${CLI_REF} 缺子命令 "${sub}" 的小节（## prune ${sub}）——usage 全集必须覆盖`);
    }
  }
  // 文档自己声明的子命令不能是 usage 里没有的
  for (const sub of refSections) {
    if (!usageSubSet.has(sub)) {
      problems.push(`${CLI_REF} 章节 "## prune ${sub}" 不在 main.rs usage() 里（实际: ${USAGE_SUBCOMMANDS.join(', ')}）`);
    }
  }

  // D2. 命令对账：文档里会被用户照着敲的 `prune ...`（shell 行 + 反引号命令），
  //     子命令与 flag 必须能过 usage 的账。
  //     token 预处理：剥掉 [ ] / | 包装（usage 写法 `[--json]`、表格里的
  //     `claude\|cursor\|…`），<占位符> 一律跳过。
  const clean = (tk) => tk.replace(/[\[\]|]/g, '');
  const refCommands = [];
  for (const m of refText.matchAll(/`prune ([^`]+)`/g)) refCommands.push(m[1]);
  for (const line of refText.split('\n')) {
    const s = line.trim();
    if (s.startsWith('prune ') && !s.includes('```')) refCommands.push(s);
  }
  for (const raw of refCommands) {
    let tokens = raw.split(/\s+/).map(clean).filter((t) => t && !t.startsWith('<'));
    // 反引号捕获不带 "prune" 前缀，shell 行带——统一剥掉
    if (tokens[0] === 'prune') tokens = tokens.slice(1);
    if (tokens.length < 1) continue;
    const sub = tokens[0];
    if (sub.startsWith('-')) {
      // 裸 flag 形态（prune --version / --help）：按 usage 子串校验
      if (usageText && !usageText.includes(sub)) {
        problems.push(`${CLI_REF} 用了 usage 里不存在的 flag 子命令: ${raw}`);
      }
      continue;
    }
    if (!usageSubSet.has(sub)) {
      problems.push(
        `${CLI_REF} 子命令 "${sub}" 不在 main.rs usage() 里（实际: ${USAGE_SUBCOMMANDS.join(', ')}）: prune ${raw}`,
      );
      continue;
    }
    for (const tk of tokens.slice(1)) {
      if (tk.startsWith('-') && !usageFlags.has(tk)) {
        problems.push(`${CLI_REF} flag "${tk}" 不在 main.rs usage() 里: prune ${raw}`);
      }
    }
  }
}

// ---------------------------------------------------------------------------
// 检查 E：退出码三方对账（cli-reference 表 ↔ ExitCode 枚举 ↔ usage 退出码行）
// ---------------------------------------------------------------------------

// E1. main.rs 的 ExitCode 枚举成员。枚举声明里变体不带 `ExitCode::` 前缀
//     （`Ok = 0,`），必须先取 enum 块再抠成员；`ExitCode::ALL` 表里的
//     `(ExitCode::Ok, "正常")` 是 usage 派生行的数据源，一起对账。
const enumBlockStart = cliSource.indexOf('pub enum ExitCode');
const enumBlockEnd = cliSource.indexOf('}', enumBlockStart);
const enumCodes = new Map();
const enumLabels = new Map();
if (enumBlockStart === -1 || enumBlockEnd === -1) {
  problems.push('main.rs 里提取不到 ExitCode 枚举（检查 E 解析失效）');
} else {
  const enumBlock = cliSource.slice(enumBlockStart, enumBlockEnd);
  for (const m of enumBlock.matchAll(/(\w+)\s*=\s*(\d+)/g)) enumCodes.set(m[2], m[1]);
  for (const m of cliSource.matchAll(/\(ExitCode::(\w+),\s*"([^"]*)"\)/g)) {
    enumLabels.set(m[1], m[2]);
  }
  for (const [code, variant] of enumCodes) {
    if (!enumLabels.has(variant)) {
      problems.push(`ExitCode::${variant}（=${code}）没有进 ALL 表——usage 派生行会漏掉它`);
    }
  }
  // usage 的退出码行由 exit_code_line() 派生（W1-DP-2），不是字面量——
  // 对账对象是 ALL 表（上面已查）+ usage 里必须有派生调用
  if (!usageText.includes('exit_code_line()')) {
    problems.push('main.rs usage() 缺 exit_code_line() 派生调用（退出码行必须由枚举生成，不许手抄）');
  }
}
if (enumCodes.size === 0) {
  problems.push('main.rs 里提取不到 ExitCode 枚举成员（检查 E 解析失效）');
}

// E3. cli-reference 退出码总表 ↔ 枚举，双向相等
if (refText) {
  const tableIdx = refText.indexOf('## 退出码总表');
  const tableText = tableIdx !== -1 ? refText.slice(tableIdx, refText.indexOf('## ', tableIdx + 4)) : '';
  const docCodes = new Set(
    [...tableText.matchAll(/^\|\s*(\d+)\s*\|/gm)].map((m) => m[1]),
  );
  if (docCodes.size === 0) {
    problems.push(`${CLI_REF}「退出码总表」提取不到码（检查 E 解析失效）`);
  }
  const missingInDoc = [...enumCodes.keys()].filter((c) => !docCodes.has(c));
  const extraInDoc = [...docCodes].filter((c) => !enumCodes.has(c));
  if (missingInDoc.length) {
    problems.push(`${CLI_REF} 退出码表缺枚举成员: ${missingInDoc.join(', ')}`);
  }
  if (extraInDoc.length) {
    problems.push(`${CLI_REF} 退出码表多出枚举之外的码: ${extraInDoc.join(', ')}`);
  }
}

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
    `；执行类动词显式拒绝 ${rejectedVerbs.length} 个（${rejectedVerbs.join(', ')}）` +
    `；编辑器配方 ${EDITOR_DOCS.length} 份命令对账一致（子命令 ${USAGE_SUBCOMMANDS.join('/')}）` +
    `；cli-reference 对账一致（${[...new Set(USAGE_SUBCOMMANDS)].length} 子命令 ↔ ${CLI_REF}，` +
    `退出码 ${enumCodes.size} 个文档↔枚举双向相等）`,
);
