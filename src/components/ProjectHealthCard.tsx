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
import type {
  ArchiveEntry,
  ArchivePlan,
  ArchiveProgress,
  ArchiveStage,
  ProjectHealth,
  ProjectVerdict,
  RestorePreview,
} from "../types/disk";
import { Badge, type Tone } from "./primitives";

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

/** 归档确认对话框的三态（f14：三个动作都必须先预览再确认） */
type DialogState =
  | { kind: "archive"; plan: ArchivePlan }
  | { kind: "restore"; preview: RestorePreview; overwrite: boolean }
  | { kind: "delete"; entry: ArchiveEntry };

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
  /** 已归档的项目根：归档后按钮立即消失，不依赖下一轮扫描 */
  const [archivedRoots, setArchivedRoots] = useState<Set<string>>(new Set());
  const [dialog, setDialog] = useState<DialogState | null>(null);
  const [busy, setBusy] = useState(false);
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
    setProgress(null);
    setCanceling(false);
    cancelRequested.current = false;
    startedAt.current = Date.now();
    try {
      if (d.kind === "archive") {
        const entry = await archiveCreate(d.plan.projectRoot, true);
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
                <span className="w-16 shrink-0 text-right font-mono text-[11.5px] tabular-nums text-ink">
                  {formatBytes(p.depsBytes)}
                </span>
              </button>
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

      <p className="mt-1.5 text-[10px] leading-relaxed text-ink-faint">
        {t("disk.project.footnote")}
      </p>

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
                  busy || (dialog.kind === "restore" && dialog.preview.conflict && !dialog.overwrite)
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
