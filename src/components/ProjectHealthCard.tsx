import { useEffect, useRef, useState } from "react";

import { listen } from "@tauri-apps/api/event";

import { useI18n } from "../i18n/store";
import type { I18n } from "../i18n";
import type { NoParamKey } from "../i18n/types";
import { formatBytes, formatDayClock } from "../lib/format";
import {
  archiveCancel,
  archiveCreate,
  archiveDelete,
  archiveList,
  archivePlan,
  archiveRestore,
  archiveRestorePreview,
  isTauri,
  revealPath,
} from "../lib/ipc";
import { useDialogA11y } from "../lib/dialogA11y";
import { projectTrends } from "../lib/ipc";
import type {
  ArchiveEntry,
  ArchivePlan,
  ArchiveProgress,
  ArchiveStage,
  ConfigNote,
  ProjectHealth,
  ProjectTrend,
  ProjectVerdict,
  RestorePreview,
} from "../types/disk";
import { useDiskStore } from "../stores/diskStore";
import { Badge, type Tone } from "./primitives";
import { Sparkline } from "./Sparkline";

/** 与 prune-core PROJECT_STALE_DAYS 对齐：依赖 60 天没重装即「久未重装」 */
const STALE_DAYS = 60;

const VERDICT_META: Record<
  ProjectVerdict,
  { tone: Tone; labelKey: NoParamKey; tipKey: NoParamKey }
> = {
  zombie: {
    tone: "danger",
    labelKey: "disk.project.verdict.zombie",
    tipKey: "disk.project.verdict.zombieTip",
  },
  heavy: {
    tone: "warn",
    labelKey: "disk.project.verdict.heavy",
    tipKey: "disk.project.verdict.heavyTip",
  },
  stale: {
    tone: "warn",
    labelKey: "disk.project.verdict.stale",
    tipKey: "disk.project.verdict.staleTip",
  },
  healthy: {
    tone: "neutral",
    labelKey: "disk.project.verdict.healthy",
    tipKey: "disk.project.verdict.healthyTip",
  },
};

/** 进度阶段 → 文案 key（plan 阶段没有总量，走不确定态） */
const STAGE_KEY: Record<ArchiveStage, NoParamKey> = {
  plan: "disk.archive.stagePlan",
  pack: "disk.archive.stagePack",
  verify: "disk.archive.stageVerify",
  trash: "disk.archive.stageTrash",
};

function installLabel(days: number, t: I18n["t"]): string {
  if (days <= 0) return t("disk.project.installToday");
  if (days === 1) return t("disk.project.installYesterday");
  if (days < 30) return t("disk.project.installDays", { n: days });
  if (days < 365) return t("disk.project.installMonths", { n: Math.round(days / 30) });
  return t("disk.project.installYears", { n: (days / 365).toFixed(1) });
}

function activityLabel(days: number | null, t: I18n["t"]): string {
  if (days == null) return t("disk.project.activityNone");
  if (days <= 0) return t("disk.project.activityToday");
  if (days === 1) return t("disk.project.activityYesterday");
  if (days < 30) return t("disk.project.activityDays", { n: days });
  if (days < 365) return t("disk.project.activityMonths", { n: Math.round(days / 30) });
  return t("disk.project.activityYears", { n: (days / 365).toFixed(1) });
}

// ---------------------------------------------------------------- W2/D-08 趋势

/** delta 的语义色：涨 = 警示（磁盘只会越涨越贵），缩 = 强调色（好消息），其余灰 */
function deltaTone(tr: ProjectTrend): "warn" | "accent" | "faint" {
  if (tr.deltaBytes == null || tr.deltaBytes === 0) return "faint";
  return tr.deltaBytes > 0 ? "warn" : "accent";
}

/** 两个 YYYY-MM-DD 之间的天数（同日 = 0）。解析失败按 1 算，不显示 0 天 */
function daySpan(from: string, to: string): number {
  const a = Date.parse(from);
  const b = Date.parse(to);
  if (Number.isNaN(a) || Number.isNaN(b)) return 1;
  return Math.max(1, Math.round((b - a) / 86_400_000));
}

/**
 * 口径纪律（W2/D-08 §5.3，测试在 Rust 侧、文案义务在这边）：
 * - `deltaBytes == null`（样本 < 2）→ 「数据不足（N 天）」，绝不显示 0；
 * - 首末间隔写实际天数——窗口是 30 天但只有 3 个采样日时，标签写「3 天」，
 *   不许写「30 天涨了 X」（那是在替缺失的日子作证）。
 */
function trendDeltaLabel(tr: ProjectTrend, t: I18n["t"]): string {
  if (tr.deltaBytes == null || tr.first == null || tr.last == null) {
    return t("disk.project.trend.insufficient", { n: tr.samples });
  }
  const days = daySpan(tr.first.day, tr.last.day);
  const size = formatBytes(Math.abs(tr.deltaBytes));
  if (tr.deltaBytes > 0) return t("disk.project.trend.deltaUp", { days, size });
  if (tr.deltaBytes < 0) return t("disk.project.trend.deltaDown", { days, size });
  return t("disk.project.trend.deltaFlat", { days });
}

/** W2/D-09 配置事件 → 本地化文案（Rust 侧只存结构化事实，句子在这边拼） */
function configNoteText(n: ConfigNote, t: I18n["t"]): string {
  const line = n.line != null ? String(n.line) : null;
  switch (n.kind) {
    case "syntax":
      return line != null
        ? t("disk.project.config.syntaxFallback", { file: n.file, line, detail: n.detail ?? "" })
        : t("disk.project.config.syntaxFallbackNoLine", { file: n.file, detail: n.detail ?? "" });
    case "value":
      return t("disk.project.config.valueFallback", { file: n.file, line: line ?? "", detail: n.detail ?? "" });
    case "path":
      return t("disk.project.config.pathSkipped", { file: n.file, detail: n.detail ?? "" });
    default:
      // ignored 单独聚合计数，不逐条铺开
      return "";
  }
}

/** 归档确认对话框的三态（f14：三个动作都必须先预览再确认） */
type DialogState =
  | { kind: "archive"; plan: ArchivePlan }
  | { kind: "restore"; preview: RestorePreview; overwrite: boolean }
  | { kind: "delete"; entry: ArchiveEntry };

/** R8b 熔断阈值的前端镜像（真实闸门在 prune-core::archive::breaker_error，由字节定） */
const BREAKER_BYTES = 20 * 1024 * 1024 * 1024;

/**
 * F9 项目健康卡（M24）+ M30 休眠归档（f14）。
 *
 * 体检部分只陈述事实、不代删；归档是**独立通道**：只打包依赖目录
 * （node_modules），校验通过才删原件，归档区可完整还原——与清理通道
 * （白名单 + 废纸篓）是两套语义，互不替代。
 */
export function ProjectHealthCard({ projects }: { projects: ProjectHealth[] }) {
  const [toast, setToast] = useState<string | null>(null);
  const [entries, setEntries] = useState<ArchiveEntry[]>([]);
  /** W2/D-08：root → 趋势（进入页面拉一次账本，只读不触发扫描） */
  const [trends, setTrends] = useState<Map<string, ProjectTrend>>(new Map());
  /** 趋势弹窗当前打开的项目 */
  const [trendOpen, setTrendOpen] = useState<ProjectTrend | null>(null);
  /** 已归档的项目根：归档后按钮立即消失，不依赖下一轮扫描 */
  const [archivedRoots, setArchivedRoots] = useState<Set<string>>(new Set());
  const [dialog, setDialog] = useState<DialogState | null>(null);
  const [busy, setBusy] = useState(false);
  /** R8b：超 20 GB 归档的二次确认勾选（对话框打开即重置，不残留上次会话） */
  const [breakerOk, setBreakerOk] = useState(false);
  /** 正在统计的项目根（归档预览前的 0.7 s，冷缓存更久，必须有反馈） */
  const [planning, setPlanning] = useState<string | null>(null);
  const [progress, setProgress] = useState<ArchiveProgress | null>(null);
  const [canceling, setCanceling] = useState(false);
  /** 进度起点：算 ETA 用，不参与渲染决策 */
  const startedAt = useRef<number | null>(null);
  /** 本次任务是否由用户点了取消：决定失败时给「已取消」还是原始错误 */
  const cancelRequested = useRef(false);
  const { t, locale } = useI18n();
  const dialogRef = useDialogA11y<HTMLDivElement>(dialog != null);
  /** W2/D-08 趋势弹窗的 a11y（独立于归档弹窗：两个 dialog 不能共用焦点陷阱） */
  const trendDialogRef = useDialogA11y<HTMLDivElement>(trendOpen != null);
  /** W2/D-09：扫描报告里的 prune.toml 配置事件 */
  const configNotes: ConfigNote[] = useDiskStore((s) => s.report)?.configNotes ?? [];
  const ignoredCount = configNotes.filter((n) => n.kind === "ignored").length;

  // 提示条 3 s 自走：Finder 打开失败是一次性事件，不该常驻在左下角
  useEffect(() => {
    if (!toast) return;
    const timer = window.setTimeout(() => setToast(null), 3000);
    return () => window.clearTimeout(timer);
  }, [toast]);

  // 归档账本：进桌面端就拉一次（浏览器预览没有 IPC，静默跳过）
  useEffect(() => {
    if (!isTauri()) return;
    archiveList()
      .then((list) => {
        setEntries(list);
        setArchivedRoots(new Set(list.map((e) => e.projectRoot)));
      })
      .catch(() => setEntries([]));
  }, []);

  // W2/D-08：30 天趋势。拉不到（库刚建 / 扫描没跑过）就静默保持空——
  // 没有趋势行就没有 delta 标签，绝不显示 0 变化
  useEffect(() => {
    if (!isTauri()) return;
    let alive = true;
    projectTrends(30)
      .then((list) => {
        if (!alive) return;
        setTrends(new Map(list.map((tr) => [tr.root, tr])));
      })
      .catch(() => {
        if (alive) setTrends(new Map());
      });
    return () => {
      alive = false;
    };
  }, []);

  // 归档进度：后端按 200 ms 节流推事件（十几万条目逐条发会把渲染线程打满）
  useEffect(() => {
    if (!isTauri()) return;
    const un = listen<ArchiveProgress>("archive://progress", (e) => setProgress(e.payload));
    return () => {
      void un.then((fn) => fn());
    };
  }, []);

  useEffect(() => {
    if (!dialog) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !busy) setDialog(null);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [dialog, busy]);

  // 趋势弹窗的 Escape 通道（独立弹窗独立监听，避免和归档弹窗互相干扰）
  useEffect(() => {
    if (!trendOpen) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setTrendOpen(null);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [trendOpen]);

  if (projects.length === 0) return null;

  const totalBytes = projects.reduce((acc, p) => acc + p.depsBytes, 0);
  const zombies = projects.filter((p) => p.verdict === "zombie");
  const zombieBytes = zombies.reduce((acc, p) => acc + p.depsBytes, 0);
  const multi = projects.filter((p) => p.multiManager);
  const shown = projects.slice(0, 6);
  const archiveBytes = entries.reduce((acc, e) => acc + e.bytes, 0);

  // 进度条：plan 阶段没有总量（不确定态）；ETA 用「已处理字节 / 已用时间」估
  const pct =
    progress && progress.totalBytes > 0
      ? Math.min(100, Math.round((progress.doneBytes / progress.totalBytes) * 100))
      : 0;
  const eta = (() => {
    if (!progress || !startedAt.current || progress.totalBytes <= 0) return null;
    if (progress.doneBytes <= 0) return null;
    const elapsed = (Date.now() - startedAt.current) / 1000;
    if (elapsed < 1.5) return null; // 刚开始的速率噪声太大，不显示
    const speed = progress.doneBytes / elapsed;
    const remain = (progress.totalBytes - progress.doneBytes) / speed;
    return remain >= 1 ? Math.round(remain) : null;
  })();

  const open = async (p: ProjectHealth) => {
    if (!isTauri()) {
      setToast(t("disk.err.needsDesktop"));
      return;
    }
    try {
      await revealPath(p.root);
    } catch (e) {
      setToast(String(e));
    }
  };

  /** 归档入口：先拿 dry-run 预览，再弹确认（绝不直接执行） */
  const beginArchive = async (p: ProjectHealth) => {
    if (!isTauri()) {
      setToast(t("disk.err.needsDesktop"));
      return;
    }
    if (planning || busy) return; // 防连点：统计要遍历整棵依赖树
    setPlanning(p.root);
    try {
      setDialog({ kind: "archive", plan: await archivePlan(p.root) });
    } catch (e) {
      setToast(String(e));
    } finally {
      setPlanning(null);
    }
  };

  const beginRestore = async (entry: ArchiveEntry) => {
    if (!isTauri()) {
      setToast(t("disk.err.needsDesktop"));
      return;
    }
    try {
      setDialog({ kind: "restore", preview: await archiveRestorePreview(entry.id), overwrite: false });
    } catch (e) {
      setToast(String(e));
    }
  };

  const runDialog = async (d: DialogState) => {
    setBusy(true);
    setBreakerOk(false);
    setProgress(null);
    setCanceling(false);
    cancelRequested.current = false;
    startedAt.current = Date.now();
    try {
      if (d.kind === "archive") {
        // R8b：超阈值必须带二次确认；阈值内传 true（后端对 ≤20 GB 不读这个位）
        const over = d.plan.bytes > BREAKER_BYTES;
        const entry = await archiveCreate(d.plan.projectRoot, true, over ? breakerOk : true);
        setEntries((prev) => [entry, ...prev]);
        setArchivedRoots((prev) => new Set(prev).add(entry.projectRoot));
        setToast(t("disk.archive.doneArchive", { size: formatBytes(entry.bytes) }));
      } else if (d.kind === "restore") {
        const rep = await archiveRestore(d.preview.id, true, d.overwrite);
        setArchivedRoots((prev) => {
          const next = new Set(prev);
          for (const e of entries) {
            if (e.id === d.preview.id) next.delete(e.projectRoot);
          }
          return next;
        });
        setEntries((prev) => prev.filter((e) => e.id !== d.preview.id));
        setToast(t("disk.archive.doneRestore", { files: rep.filesRestored }));
      } else {
        await archiveDelete(d.entry.id, true);
        setEntries((prev) => prev.filter((e) => e.id !== d.entry.id));
        setToast(t("disk.archive.doneDelete"));
      }
      setDialog(null);
    } catch (e) {
      setToast(cancelRequested.current ? t("disk.archive.cancelled") : String(e));
    } finally {
      setBusy(false);
      setProgress(null);
      setCanceling(false);
      cancelRequested.current = false;
      startedAt.current = null;
    }
  };

  /** 归档/恢复跑到一半的取消：不允许「关掉对话框假装取消」 */
  const requestCancel = async () => {
    if (canceling) return;
    cancelRequested.current = true;
    setCanceling(true);
    try {
      await archiveCancel();
    } catch (e) {
      setToast(String(e));
      setCanceling(false);
    }
  };

  const dialogBody = (d: DialogState): { title: string; line: string; confirmText: string } => {
    if (d.kind === "archive") {
      return {
        title: t("disk.archive.planTitle"),
        line: t("disk.archive.planLine", {
          files: d.plan.files,
          size: formatBytes(d.plan.bytes),
        }),
        confirmText: t("disk.archive.confirm"),
      };
    }
    if (d.kind === "restore") {
      return {
        title: t("disk.archive.restoreTitle"),
        line: t("disk.archive.restoreLine", {
          files: d.preview.files,
          size: formatBytes(d.preview.bytes),
          target: d.preview.target,
        }),
        confirmText: t("disk.archive.confirm"),
      };
    }
    return {
      title: t("disk.archive.deleteTitle"),
      line: t("disk.archive.deleteLine"),
      confirmText: t("disk.archive.confirm"),
    };
  };

  return (
    <section className="mt-4 rounded-xl border border-hairline-soft bg-chip-soft p-3.5">
      <div className="flex items-baseline justify-between gap-3">
        <h2 className="text-[12px] font-medium text-ink">{t("disk.project.title")}</h2>
        <span className="font-mono text-[11px] tabular-nums text-ink-dim">
          {t("disk.project.meta", { n: projects.length, size: formatBytes(totalBytes) })}
        </span>
      </div>

      <p className="mt-1 text-[10.5px] leading-relaxed text-ink-faint">
        {zombies.length > 0 ? (
          <>
            <span className="text-danger">{t("disk.project.zombies", { n: zombies.length })}</span>
            {t("disk.project.zombiesNote", {
              size: formatBytes(zombieBytes),
              days: STALE_DAYS,
            })}
          </>
        ) : (
          <>{t("disk.project.noZombies")}</>
        )}
        {multi.length > 0 ? t("disk.project.multi", { n: multi.length }) : ""}
      </p>

      <ul className="mt-2.5 space-y-0.5">
        {shown.map((p) => {
          const meta = VERDICT_META[p.verdict];
          const archived = archivedRoots.has(p.root);
          const trend = trends.get(p.root);
          return (
            <li key={p.root} className="flex items-center gap-1">
              <button
                type="button"
                onClick={() => void open(p)}
                title={t("disk.fileList.title", {
                  path: p.depsPath,
                  tip: t("disk.pathMenu.openWithTip"),
                })}
                className="flex min-w-0 flex-1 items-center gap-2 rounded-md px-1 py-1.5 text-left transition-colors hover:bg-chip-hover"
              >
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-[11.5px] text-ink">{p.name}</span>
                  <span className="mt-0.5 block truncate text-[10px] text-ink-faint">
                    {t("disk.project.line", {
                      install: installLabel(p.depsAgeDays, t),
                      activity: activityLabel(p.activityAgeDays, t),
                    })}
                  </span>
                </span>
                {p.multiManager ? (
                  <Badge
                    tone="info"
                    title={t("disk.project.multiTip", { files: p.lockfiles.join(" / ") })}
                  >
                    {t("disk.project.multiBadge")}
                  </Badge>
                ) : null}
                <Badge tone={meta.tone} title={t(meta.tipKey)}>
                  {t(meta.labelKey)}
                </Badge>
                <span className="w-20 shrink-0 text-right">
                  <span className="block font-mono text-[11.5px] tabular-nums text-ink">
                    {formatBytes(p.depsBytes)}
                  </span>
                  {/* W2/D-08：delta 标签。null = 样本不足 → 「数据不足」，不显示 0 */}
                  {trend ? (
                    <span
                      className={`mt-0.5 block text-[9px] leading-tight ${
                        deltaTone(trend) === "warn"
                          ? "text-warn"
                          : deltaTone(trend) === "accent"
                            ? "text-accent"
                            : "text-ink-faint"
                      }`}
                    >
                      {trendDeltaLabel(trend, t)}
                    </span>
                  ) : null}
                </span>
              </button>
              {trend ? (
                <button
                  type="button"
                  onClick={() => setTrendOpen(trend)}
                  title={t("disk.project.trend.openTip")}
                  className="shrink-0 rounded-ctl border border-hairline-soft bg-chip px-1.5 py-0.5 text-[10px] text-ink-dim transition-colors hover:bg-chip-hover hover:text-ink"
                >
                  {t("disk.project.trend.openTip")}
                </button>
              ) : null}
              {p.verdict !== "healthy" && !archived ? (
                <button
                  type="button"
                  onClick={() => void beginArchive(p)}
                  disabled={planning != null || busy}
                  title={t("disk.archive.entryTip")}
                  className="shrink-0 rounded-ctl border border-hairline-soft bg-chip px-1.5 py-0.5 text-[10px] text-ink-dim transition-colors hover:bg-chip-hover hover:text-ink disabled:opacity-40 disabled:hover:bg-chip"
                >
                  {planning === p.root ? t("disk.archive.measuring") : t("disk.archive.entry")}
                </button>
              ) : null}
            </li>
          );
        })}
        {projects.length > shown.length ? (
          <li className="px-1 pt-1 text-[10px] text-ink-faint">
            {t("disk.project.more", { n: projects.length - shown.length })}
          </li>
        ) : null}
      </ul>

      {/* M30 归档区：已归档的依赖可恢复 / 删除（删除也先确认） */}
      <div className="mt-3 border-t border-hairline-soft pt-2">
        <div className="flex items-baseline justify-between gap-3">
          <h3 className="text-[11px] font-medium text-ink">{t("disk.archive.title")}</h3>
          <span className="font-mono text-[10.5px] tabular-nums text-ink-dim">
            {t("disk.archive.meta", { n: entries.length, size: formatBytes(archiveBytes) })}
          </span>
        </div>
        {entries.length === 0 ? (
          <p className="mt-1 text-[10px] text-ink-faint">{t("disk.archive.empty")}</p>
        ) : (
          <ul className="mt-1.5 space-y-0.5">
            {entries.map((e) => (
              <li key={e.id} className="flex items-center gap-2 px-1 py-1">
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-[11px] text-ink">{e.projectName}</span>
                  <span className="mt-0.5 block truncate font-mono text-[9.5px] tabular-nums text-ink-faint">
                    {t("disk.archive.listLine", {
                      size: formatBytes(e.bytes),
                      files: e.files,
                      date: formatDayClock(e.archivedAt, locale),
                    })}
                  </span>
                </span>
                <button
                  type="button"
                  onClick={() => void beginRestore(e)}
                  className="shrink-0 rounded-ctl border border-hairline-soft bg-chip px-1.5 py-0.5 text-[10px] text-ink-dim transition-colors hover:bg-chip-hover hover:text-ink"
                >
                  {t("disk.archive.restore")}
                </button>
                <button
                  type="button"
                  onClick={() => setDialog({ kind: "delete", entry: e })}
                  className="shrink-0 rounded-ctl border border-danger/30 px-1.5 py-0.5 text-[10px] text-danger transition-colors hover:bg-danger/[0.12]"
                >
                  {t("disk.archive.delete")}
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>

      {/* W2/D-09：prune.toml 配置事件。被忽略的项目不在列表里（不进账本），
          所以这里只报「跳过了几个 + 哪些配置没生效」——沉默的忽略不是透明 */}
      {ignoredCount > 0 || configNotes.length > 0 ? (
        <div className="mt-2 rounded-md bg-chip/70 px-2 py-1.5">
          {ignoredCount > 0 ? (
            <p className="text-[10.5px] text-ink-dim">
              {t("disk.project.config.ignored", { n: ignoredCount })}
            </p>
          ) : null}
          {configNotes
            .filter((n) => n.kind !== "ignored")
            .slice(0, 4)
            .map((n, i) => (
              <p
                key={`${n.file}-${n.line ?? "f"}-${i}`}
                className="mt-0.5 truncate text-[9.5px] text-ink-faint"
                title={configNoteText(n, t)}
              >
                {configNoteText(n, t)}
              </p>
            ))}
        </div>
      ) : null}

      <p className="mt-1.5 text-[10px] leading-relaxed text-ink-faint">
        {t("disk.project.footnote")}
      </p>

      {/* W2/D-08 趋势弹窗：火花线 + 采样点 + 口径说明（复用归档弹窗的版式） */}
      {trendOpen ? (
        <div className="fixed inset-0 z-[90] flex items-center justify-center p-6">
          <div
            className="prune-fade-in absolute inset-0 bg-black/45 backdrop-blur-[2px]"
            onClick={() => setTrendOpen(null)}
          />
          <div
            ref={trendDialogRef}
            role="dialog"
            aria-modal="true"
            aria-label={t("disk.project.trend.modalTitle", { name: trendOpen.name })}
            className="relative w-full max-w-sm rounded-xl border border-hairline bg-surface-overlay p-4 shadow-2xl"
          >
            <p className="text-[12.5px] font-medium text-ink">
              {t("disk.project.trend.modalTitle", { name: trendOpen.name })}
            </p>
            <p className="mt-1 text-[11px] text-ink-dim">
              {trendDeltaLabel(trendOpen, t)}
              {trendOpen.peakBytes != null
                ? ` · ${t("disk.project.trend.peak", { size: formatBytes(trendOpen.peakBytes) })}`
                : ""}
            </p>
            <div className="mt-2.5">
              <Sparkline
                values={trendOpen.points.map((pt) => pt.depsBytes)}
                tone="accent"
                height={56}
                baselineZero
              />
            </div>
            <div className="mt-1.5 flex items-baseline justify-between text-[10px] text-ink-faint">
              <span>{trendOpen.first?.day ?? "—"}</span>
              <span>{t("disk.project.trend.points", { n: trendOpen.samples })}</span>
              <span>{trendOpen.last?.day ?? "—"}</span>
            </div>
            <p className="mt-2 text-[10px] leading-relaxed text-ink-faint">
              {t("disk.project.trend.windowNote")}
            </p>
            <div className="mt-3 flex justify-end">
              <button
                type="button"
                onClick={() => setTrendOpen(null)}
                className="rounded-ctl bg-ink px-2.5 py-1 text-[11px] text-surface transition-opacity hover:opacity-85"
              >
                {t("common.action.close")}
              </button>
            </div>
          </div>
        </div>
      ) : null}

      {dialog ? (
        <div className="fixed inset-0 z-[90] flex items-center justify-center p-6">
          <div
            className="prune-fade-in absolute inset-0 bg-black/45 backdrop-blur-[2px]"
            onClick={() => (busy ? undefined : setDialog(null))}
          />
          <div
            ref={dialogRef}
            role="dialog"
            aria-modal="true"
            aria-label={dialogBody(dialog).title}
            className="relative w-full max-w-sm rounded-xl border border-hairline bg-surface-overlay p-4 shadow-2xl"
          >
            <p className="text-[12.5px] font-medium text-ink">{dialogBody(dialog).title}</p>
            <p className="mt-1.5 text-[11px] leading-relaxed text-ink-dim">
              {dialogBody(dialog).line}
            </p>
            {dialog.kind === "restore" && dialog.preview.conflict ? (
              <div className="mt-2 rounded-md border border-danger/30 bg-danger/[0.08] p-2">
                <p className="text-[10.5px] leading-relaxed text-danger">{t("disk.archive.conflict")}</p>
                <label className="mt-1.5 flex items-center gap-1.5 text-[10.5px] text-ink-dim">
                  <input
                    type="checkbox"
                    checked={dialog.overwrite}
                    onChange={(e) =>
                      setDialog({ ...dialog, overwrite: e.currentTarget.checked })
                    }
                  />
                  {t("disk.archive.overwrite")}
                </label>
              </div>
            ) : null}
            {dialog.kind === "archive" && dialog.plan.bytes > BREAKER_BYTES ? (
              <div className="mt-2 rounded-md border border-warn/30 bg-warn/[0.08] p-2">
                <p className="text-[10.5px] leading-relaxed text-warn">{t("disk.archive.breakerNote")}</p>
                <label className="mt-1.5 flex items-center gap-1.5 text-[10.5px] text-ink-dim">
                  <input
                    type="checkbox"
                    checked={breakerOk}
                    onChange={(e) => setBreakerOk(e.currentTarget.checked)}
                  />
                  {t("disk.archive.breakerCheck")}
                </label>
              </div>
            ) : null}
            {busy ? (
              <div className="mt-2.5" aria-live="polite">
                <div className="flex items-baseline justify-between gap-2 text-[10px] text-ink-dim">
                  <span>{t(progress ? STAGE_KEY[progress.stage] : "disk.archive.stagePlan")}</span>
                  <span className="font-mono tabular-nums text-ink-faint">
                    {progress && progress.totalBytes > 0
                      ? `${pct}% · ${t("disk.archive.progressLine", {
                          done: formatBytes(progress.doneBytes),
                          total: formatBytes(progress.totalBytes),
                        })}`
                      : t("disk.archive.measuring")}
                  </span>
                </div>
                <div className="mt-1 h-1 overflow-hidden rounded-full bg-ink/10">
                  <div
                    className={`h-full rounded-full bg-ink transition-[width] duration-200 ${
                      progress && progress.totalBytes > 0 ? "" : "animate-pulse opacity-40"
                    }`}
                    style={{ width: `${pct}%` }}
                  />
                </div>
                {eta != null ? (
                  <p className="mt-1 font-mono text-[9.5px] tabular-nums text-ink-faint">
                    {t("disk.archive.eta", { sec: eta })}
                  </p>
                ) : null}
              </div>
            ) : null}
            <div className="mt-3 flex justify-end gap-2">
              <button
                type="button"
                onClick={() => (busy ? void requestCancel() : setDialog(null))}
                disabled={canceling}
                className="rounded-ctl border border-hairline-soft px-2.5 py-1 text-[11px] text-ink-dim transition-colors hover:bg-chip disabled:opacity-40"
              >
                {busy
                  ? canceling
                    ? t("disk.archive.canceling")
                    : t("disk.archive.cancelTask")
                  : t("disk.archive.cancel")}
              </button>
              <button
                type="button"
                onClick={() => void runDialog(dialog)}
                disabled={
                  busy ||
                  (dialog.kind === "restore" && dialog.preview.conflict && !dialog.overwrite) ||
                  (dialog.kind === "archive" && dialog.plan.bytes > BREAKER_BYTES && !breakerOk)
                }
                className="rounded-ctl bg-ink px-2.5 py-1 text-[11px] text-surface transition-opacity hover:opacity-85 disabled:opacity-40"
              >
                {busy ? t("disk.archive.busy") : dialogBody(dialog).confirmText}
              </button>
            </div>
          </div>
        </div>
      ) : null}

      {toast ? (
        <div className="fixed bottom-16 left-5 z-50 rounded-md border border-danger/30 bg-danger/[0.12] px-3 py-2 text-[10.5px] text-danger shadow-xl">
          {toast}
        </div>
      ) : null}
    </section>
  );
}
