import { useEffect, useState } from "react";

import { formatBytes } from "../lib/format";
import { isTauri, revealPath } from "../lib/ipc";
import type { ProjectHealth, ProjectVerdict } from "../types/disk";
import { Badge, type Tone } from "./primitives";

/** 与 prune-core PROJECT_STALE_DAYS 对齐：依赖 60 天没重装即「久未重装」 */
const STALE_DAYS = 60;

const VERDICT_META: Record<ProjectVerdict, { label: string; tone: Tone; title: string }> = {
  zombie: {
    label: "僵尸依赖",
    tone: "danger",
    title: "依赖 60 天以上没重装，项目最后活动也在 60 天前（或查不到活动痕迹）",
  },
  heavy: { label: "偏胖", tone: "warn", title: "依赖体积 ≥ 1.5 GB，装的东西比用到的多" },
  stale: { label: "久未重装", tone: "warn", title: "依赖 60 天以上没重装，但项目近期还在动" },
  healthy: { label: "健康", tone: "neutral", title: "依赖半年内重装过、体积正常" },
};

function installLabel(days: number): string {
  if (days <= 0) return "今天装过";
  if (days === 1) return "昨天装过";
  if (days < 30) return `${days} 天前装过`;
  if (days < 365) return `${Math.round(days / 30)} 个月前装过`;
  return `${(days / 365).toFixed(1)} 年前装过`;
}

function activityLabel(days: number | null): string {
  if (days == null) return "没找到活动痕迹";
  if (days <= 0) return "今天动过";
  if (days === 1) return "昨天动过";
  if (days < 30) return `${days} 天前动过`;
  if (days < 365) return `${Math.round(days / 30)} 个月前动过`;
  return `${(days / 365).toFixed(1)} 年前动过`;
}

/**
 * F9 项目健康卡（M24）：把「N 个 node_modules 共 X GB」拆成按项目的依赖账本。
 *
 * 只做体检：不做单项目清理入口（存量清理通道一次列出全部 node_modules，
 * 单独剪某一个需要新 API，不在本期）——卡里所有数字都可追溯到一次 stat，
 * 不显示「预计可省」这类推断值。
 */
export function ProjectHealthCard({ projects }: { projects: ProjectHealth[] }) {
  const [toast, setToast] = useState<string | null>(null);

  // 提示条 3 s 自走：Finder 打开失败是一次性事件，不该常驻在左下角
  useEffect(() => {
    if (!toast) return;
    const timer = window.setTimeout(() => setToast(null), 3000);
    return () => window.clearTimeout(timer);
  }, [toast]);

  if (projects.length === 0) return null;

  const totalBytes = projects.reduce((acc, p) => acc + p.depsBytes, 0);
  const zombies = projects.filter((p) => p.verdict === "zombie");
  const zombieBytes = zombies.reduce((acc, p) => acc + p.depsBytes, 0);
  const multi = projects.filter((p) => p.multiManager);
  const shown = projects.slice(0, 6);

  const open = async (p: ProjectHealth) => {
    if (!isTauri()) {
      setToast("浏览器预览模式无法打开 Finder");
      return;
    }
    try {
      await revealPath(p.root);
    } catch (e) {
      setToast(String(e));
    }
  };

  return (
    <section className="mt-4 rounded-xl border border-hairline-soft bg-white/[0.02] p-3.5">
      <div className="flex items-baseline justify-between gap-3">
        <h2 className="text-[12px] font-medium text-ink">项目体检</h2>
        <span className="font-mono text-[11px] tabular-nums text-ink-dim">
          {projects.length} 个项目 · 依赖 {formatBytes(totalBytes)}
        </span>
      </div>

      <p className="mt-1 text-[10.5px] leading-relaxed text-ink-faint">
        {zombies.length > 0 ? (
          <>
            <span className="text-danger">{zombies.length} 个僵尸依赖</span>（{formatBytes(zombieBytes)} ·
            依赖与项目都 {STALE_DAYS} 天没动）
          </>
        ) : (
          <>没有依赖与项目双双停摆的项目</>
        )}
        {multi.length > 0 ? ` · ${multi.length} 个项目锁文件并存` : ""}
      </p>

      <ul className="mt-2.5 space-y-0.5">
        {shown.map((p) => {
          const meta = VERDICT_META[p.verdict];
          return (
            <li key={p.root}>
              <button
                type="button"
                onClick={() => void open(p)}
                title={`${p.depsPath}\n点击在 Finder 中打开项目目录`}
                className="flex w-full items-center gap-2 rounded-md px-1 py-1.5 text-left transition-colors hover:bg-white/[0.04]"
              >
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-[11.5px] text-ink">{p.name}</span>
                  <span className="mt-0.5 block truncate text-[10px] text-ink-faint">
                    {installLabel(p.depsAgeDays)} · 项目{activityLabel(p.activityAgeDays)}
                  </span>
                </span>
                {p.multiManager ? (
                  <Badge tone="info" title={`同时存在 ${p.lockfiles.join(" / ")} 锁文件：依赖树被多个管理器接管过`}>
                    多锁
                  </Badge>
                ) : null}
                <Badge tone={meta.tone} title={meta.title}>
                  {meta.label}
                </Badge>
                <span className="w-16 shrink-0 text-right font-mono text-[11.5px] tabular-nums text-ink">
                  {formatBytes(p.depsBytes)}
                </span>
              </button>
            </li>
          );
        })}
        {projects.length > shown.length ? (
          <li className="px-1 pt-1 text-[10px] text-ink-faint">
            … 另有 {projects.length - shown.length} 个项目在「项目依赖」明细里
          </li>
        ) : null}
      </ul>

      <p className="mt-1.5 text-[10px] leading-relaxed text-ink-faint">
        口径：依赖年龄 = node_modules 目录上次变动；项目活动 = package.json / 锁文件 /
        .git/index 的最新改动。只做体检，不代删——清理走上方「项目依赖」分组的逐项确认。
      </p>

      {toast ? (
        <div className="fixed bottom-16 left-5 z-50 rounded-md border border-danger/30 bg-danger/[0.12] px-3 py-2 text-[10.5px] text-danger shadow-xl">
          {toast}
        </div>
      ) : null}
    </section>
  );
}
