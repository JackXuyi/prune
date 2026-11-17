// W1-2 体检样例报告 · Step 1 collect
// 只消费现有 CLI 的 --json 输出；单条失败不中止——失败是内容，不是异常。
// 不加探测、不采 LAN、默认不采 net_sites（见 docs/w1-report-sample-plan.md §3.1）。
//
// 关键约定（勿改）：**退出码非 0 不等于没有数据**。
// doctor 在「全部选中域都不可读」时打印完整 JSON 后才返回 3/4（doctor.rs:exit_for），
// 此时 stdout 里正带着 findings[].unreadable 的原因——恰好是报告最该呈现的内容。
// 因此判定顺序是：先看能否解析出契约 JSON，再看退出码。

import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { join } from "node:path";

// 契约 v1（crates/prune-cli/cli-contract.v1.json）的退出码语义
export const EXIT_CODES = {
  0: "正常",
  2: "用法错误",
  3: "权限不足",
  4: "探测失败",
  10: "监测警告",
};

export const EXPECTED_SCHEMA_VERSION = 1;

/** 在仓库内解析 CLI 二进制：PRUNE_BIN > target/release > target/debug > PATH */
export function resolvePruneBin(root) {
  if (process.env.PRUNE_BIN && existsSync(process.env.PRUNE_BIN)) {
    return process.env.PRUNE_BIN;
  }
  for (const rel of ["target/release/prune", "target/debug/prune"]) {
    const p = join(root, rel);
    if (existsSync(p)) return p;
  }
  return "prune"; // PATH 里的可能是旧版，版本不匹配时 collect 会拒跑
}

function runOnce(bin, args, cwd) {
  return new Promise((resolve) => {
    const t0 = Date.now();
    const child = spawn(bin, args, { cwd, timeout: 180_000 });
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (d) => (stdout += d));
    child.stderr.on("data", (d) => (stderr += d));
    child.on("error", (err) =>
      resolve({
        exitCode: null,
        stderr: `无法启动 ${bin}: ${err.message}`,
        stdout: "",
        ms: Date.now() - t0,
      }),
    );
    child.on("close", (code) => resolve({ exitCode: code, stdout, stderr, ms: Date.now() - t0 }));
  });
}

/**
 * 跑一条 CLI 命令并解析 JSON 信封。
 * 校验 schemaVersion 与契约一致（RK-R4：字段漂移时拒跑并提示）。
 */
async function collectOne(bin, cwd, key, args) {
  const res = await runOnce(bin, args, cwd);
  const base = {
    key,
    cmd: `prune ${args.filter((a) => a !== "--json").join(" ")}`,
    ms: res.ms,
    exitCode: res.exitCode,
  };
  const exitNote =
    res.exitCode === 0 ? null : EXIT_CODES[res.exitCode] ?? `未知退出码 ${res.exitCode}`;

  // 1) 先尝试解析：退出码非 0 也可能带着完整 JSON（见文件头约定）
  let data = null;
  let parseError = null;
  try {
    // CLI 在 stdout 前可能带进度提示行，取第一个 "{" 起的 JSON
    const start = res.stdout.indexOf("{");
    if (start < 0) throw new Error("stdout 中没有 JSON");
    data = JSON.parse(res.stdout.slice(start));
  } catch (e) {
    parseError = e.message;
  }

  if (!data) {
    return {
      ...base,
      ok: false,
      reason:
        (exitNote ?? "命令未返回数据") +
        (parseError ? `（${parseError}）` : "") +
        (res.stderr ? `；stderr: ${res.stderr.trim().slice(0, 120)}` : ""),
    };
  }

  if (data.schemaVersion !== EXPECTED_SCHEMA_VERSION) {
    return {
      ...base,
      ok: false,
      reason: `schemaVersion 不匹配：期望 ${EXPECTED_SCHEMA_VERSION}，实际 ${data.schemaVersion}（契约已变更，请更新脚本）`,
    };
  }

  // 2) 拿到契约 JSON 即视为可用；退出码非 0 记为 degraded（部分/全局不可读）
  return { ...base, ok: true, degraded: res.exitCode !== 0, exitNote, data };
}

/**
 * 分两波采集：
 *   波 1（并行）：scan / mem / port / projects
 *   波 2（独占）：doctor
 * 原因：doctor 的磁盘域有 20s 超时（doctor.rs TIMEOUT），与 scan 同时遍历磁盘时
 * 会互相挤压——实测触发过一次 timeout，丢掉 disk 域读数。doctor 独占跑，它的
 * 数字就是产品单独运行时的样子。波 1 约 25s + 波 2 约 23s ≈ 50s，仍在 A1 预算内。
 */
export async function collect(root) {
  const bin = resolvePruneBin(root);
  const wave1 = [
    { key: "scan", args: ["scan", "--json"] },
    { key: "mem", args: ["mem", "--json"] },
    { key: "port", args: ["port", "--conflicts", "--json"] },
    { key: "projects", args: ["projects", "--days", "90", "--json"] },
  ];
  const wave2 = [{ key: "doctor", args: ["doctor", "--json"] }];

  const run = (jobs) => Promise.all(jobs.map((job) => collectOne(bin, root, job.key, job.args)));
  const [r1, r2] = [await run(wave1), await run(wave2)];

  const byKey = new Map([...r1, ...r2].map((r) => [r.key, r]));
  const order = ["doctor", "scan", "mem", "port", "projects"]; // 报告中的固定顺序
  const items = {};
  order.forEach((k) => (items[k] = byKey.get(k)));
  return { bin, items, waves: 2 };
}
