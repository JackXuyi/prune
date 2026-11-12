import { useEffect, useMemo, useRef, useState } from "react";

import { useI18n } from "../i18n/store";
import type { I18n } from "../i18n";
import { formatBytes, formatPercent, formatRelative } from "../lib/format";
import { useDialogA11y } from "../lib/dialogA11y";
import { isTauri, reclaimChildren, revealPath } from "../lib/ipc";
import { useDiskStore } from "../stores/diskStore";
import { StackBar } from "./charts/StackBar";
import { BarList } from "./charts/BarList";
import { RingGauge } from "./charts/RingGauge";
import { EmptyState } from "./EmptyArt";
import { ProjectHealthCard } from "./ProjectHealthCard";
import {
  cacheLabel,
  reclaimNoteText,
  LEVEL_META,
  type CacheGroup,
  type DeadweightStats,
  type DetailItem,
  type Level,
  type ReclaimChild,
  type ReclaimItem,
  type VolumeInfo,
} from "../types/disk";
import { ChevronRightIcon, FileIcon, FolderIcon, RefreshIcon } from "./icons";
import { Btn, PopMenu, toast } from "./primitives";

/** 分级固定顺序：从「零后悔」到「只给命令」，与安全分级同源 */
const TIERS: Level[] = ["l1", "l2", "l3", "l4"];

/**
 * 存储 tab（UI v5 §4.4）。
 *
 * **三段链路是唯一正确形态**（旧版画成「四个勾选框 → 一键清理」是错的）：
 * ① 分级构成（只读 + 勾选决定预览范围）
 * ② 分组预览（展开某一级，逐组勾选/右键定位）
 * ③ 底部抽屉确认（逐条取消勾选 + L4 二次确认 + 主按钮）
 *
 * 为什么不能一步清理：L1 是「零后悔」但 L2/L3 删除后要重装或丢状态，
 * L4 只给命令不执行。把四档压成一个按钮，等于把风险选择权从用户手里拿走。
 */
export function ReclaimBoard() {
  const report = useDiskStore((s) => s.report);
  const scanning = useDiskStore((s) => s.scanning);
  const progress = useDiskStore((s) => s.progress);
  const error = useDiskStore((s) => s.error);
  const preview = useDiskStore((s) => s.preview);
  const previewLoading = useDiskStore((s) => s.previewLoading);
  const lastReclaimed = useDiskStore((s) => s.lastReclaimed);
  const busy = useDiskStore((s) => s.busy);
  const refresh = useDiskStore((s) => s.refresh);
  const loadPreview = useDiskStore((s) => s.loadPreview);
  const clearPreview = useDiskStore((s) => s.clearPreview);
  const execute = useDiskStore((s) => s.execute);
  const clearReclaimed = useDiskStore((s) => s.clearReclaimed);
  const i18n = useI18n();
  const { t } = i18n;

  /** 已勾选的分组 key（级 ↔ 组双向同步的唯一真相） */
  const [picked, setPicked] = useState<Set<string>>(new Set());
  /** 当前展开的级 */
  const [openTier, setOpenTier] = useState<Level | null>(null);
  const [drawer, setDrawer] = useState(false);

  const groups = useMemo(() => report?.groups ?? [], [report]);

  // 清理成功 → toast「已清理 X，进入废纸篓」；用 ref 去重，避免重挂载时把旧结果再播一次
  const seenReclaim = useRef<typeof lastReclaimed>(null);
  useEffect(() => {
    if (!lastReclaimed || seenReclaim.current === lastReclaimed) return;
    seenReclaim.current = lastReclaimed;
    toast(t("disk.drawer.done", { size: formatBytes(lastReclaimed.bytes) }), "ok");
  }, [lastReclaimed, t]);

  // 报告换了（重扫/清理后）就清空选择：旧 key 可能已经不存在
  const scannedAt = report?.scannedAt;
  useEffect(() => {
    setPicked(new Set());
    setOpenTier(null);
  }, [scannedAt]);

  if (error) {
    return (
      <div className="m-4 rounded-card bg-danger/[0.08] px-3 py-2 text-[11px] text-danger">{error}</div>
    );
  }

  if (!report) {
    return (
      <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4">
        {/* 页名在顶栏（spec §1.3）；这里只留扫描状态 —— 它是状态不是标题 */}
        <p className="text-[11px] text-ink-faint">
          {scanning
            ? progress
              ? t("disk.scanningWith", { done: progress.done, total: progress.total })
              : t("disk.scanning")
            : t("common.hint.firstScan")}
        </p>
        <Btn
          className="mt-3"
          onClick={() => void refresh()}
          disabled={scanning}
          icon={<RefreshIcon size={12} className={scanning ? "animate-spin" : ""} />}
        >
          {scanning ? t("common.progress.scanning") : t("common.action.scanNow")}
        </Btn>
        {/* UI v4（§6.7）：剪刀线稿空态——等待扫描也有情绪 */}
        {!scanning ? (
          <EmptyState
            art="scan"
            title={t("disk.empty.noReportTitle")}
            description={t("disk.empty.noReportNote")}
          />
        ) : null}
      </div>
    );
  }

  const a1 = groups.find((g) => g.deadweight != null) ?? null;
  const totals = report.totals;

  /**
   * 某一级下面列出的分组。
   *
   * 关键纠偏：L1「跨平台二进制」不是一个目录，而是缓存目录里被标记为死重的
   * 那部分条目 —— 它寄居在 Yarn 缓存（level=l2）里。所以：
   *   L1 → 带 deadweight 的分组（大小取死重切片，不是整组）
   *   L2/L3/L4 → 本级没有死重切片的分组（避免同一个组在两个级里出现两次）
   */
  const groupsOfTier = (level: Level) =>
    level === "l1"
      ? groups.filter((g) => g.deadweight != null)
      : groups.filter((g) => g.level === level && g.deadweight == null);

  const pickedGroups = groups.filter((g) => picked.has(g.key));
  // 「能删多少」按分组自身口径累加（含 deadweight 时只算可删的那一块），
  // 不用 totals：totals 是「可回收」记账口径，两者会差在不可删的部分上。
  const pickedBytes = pickedGroups.reduce((n, g) => n + reclaimableOf(g), 0);

  const toggleGroup = (key: string) =>
    setPicked((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });

  const openDrawer = async () => {
    const keys = pickedGroups.map((g) => g.key);
    if (keys.length === 0) return;
    setDrawer(true);
    await loadPreview(keys);
  };

  const closeDrawer = () => {
    setDrawer(false);
    clearPreview();
  };

  return (
    <div className="relative flex min-h-0 flex-1 flex-col">
      <div className="min-h-0 flex-1 overflow-y-auto px-5 pt-3 pb-3">
        {/* 头部：可回收总额（顶栏承担页名，这里只放这一页的核心事实） */}
        <div className="flex items-start justify-between gap-4">
          <div className="min-w-0">
            <div className="flex items-baseline gap-2">
              <span className="font-mono text-[26px] leading-none font-semibold tracking-tight tabular-nums text-ink">
                <span className="mr-1 text-accent">⚡</span>
                {formatBytes(totals.reclaimable)}
              </span>
              <span className="text-[12px] text-ink-faint">{t("disk.reclaimableSuffix")}</span>
            </div>
            <p className="mt-1 text-[10px] text-ink-faint">{t("disk.subnet")}</p>
            <p className="mt-0.5 text-[10px] text-ink-faint">
              {t("disk.groups.meta", {
                n: groups.length,
                when: formatRelative(report.scannedAt, i18n.locale),
                secs: (report.scanElapsedMs / 1000).toFixed(1),
              })}
              {report.incremental ? t("disk.groups.incremental") : ""}
            </p>
          </div>
          <Btn
            className="mt-0.5"
            onClick={() => void refresh()}
            disabled={scanning}
            icon={<RefreshIcon size={12} className={scanning ? "animate-spin" : ""} />}
          >
            {scanning ? t("common.progress.scanning") : t("common.action.rescan")}
          </Btn>
        </div>

        {/* 清理完成条：「已移入废纸篓 N 项」+ 从右上浮起的 +N GB */}
        {lastReclaimed ? (
          <div className="relative mt-2.5 flex items-center gap-2.5 rounded-card bg-accent/[0.08] px-3 py-2 text-[11px] text-accent">
            {lastReclaimed.bytes > 0 ? (
              <span
                className="prune-float-up pointer-events-none absolute top-0.5 right-20 font-mono text-[13px] font-semibold tabular-nums"
                aria-hidden
              >
                +{formatBytes(lastReclaimed.bytes)}
              </span>
            ) : null}
            <span className="min-w-0 flex-1 truncate">
              {t("disk.reclaimed.moved", {
                n: lastReclaimed.moved,
                size: formatBytes(lastReclaimed.bytes),
              })}
              {lastReclaimed.skipped > 0
                ? t("disk.reclaimed.skipped", { n: lastReclaimed.skipped })
                : ""}
              {lastReclaimed.failed.length > 0
                ? t("disk.reclaimed.failed", { n: lastReclaimed.failed.length })
                : ""}
            </span>
            <button
              type="button"
              onClick={clearReclaimed}
              className="shrink-0 text-[10px] text-ink-dim transition-colors hover:text-ink"
            >
              {t("disk.reclaimed.dismiss")}
            </button>
          </div>
        ) : null}

        {/* 卷容量：可回收的 GB 需要分母才有意义 */}
        {report.volume ? (
          <VolumeCard volume={report.volume} reclaimable={totals.reclaimable} />
        ) : null}

        {/* 扫完了但确实没有可回收项 —— 「很干净」是正反馈，不是缺数据 */}
        {totals.reclaimable === 0 ? (
          <div className="mt-2.5 rounded-card border border-hairline-soft bg-surface-raised py-2">
            <EmptyState
              art="scan"
              title={t("disk.empty.cleanTitle")}
              description={t("disk.empty.cleanNote")}
            />
          </div>
        ) : null}

        {/* ① 分级构成 + ② 分组预览（同一个卡，展开即预览） */}
        {totals.reclaimable > 0 ? (
          <section className="mt-2.5 rounded-card border border-hairline-soft bg-surface-raised">
            <div className="flex items-baseline justify-between gap-3 px-3.5 pt-2.5">
              <span className="text-[11.5px] font-medium text-ink">{t("disk.stack.title")}</span>
              <span className="text-[9.5px] text-ink-faint">{t("disk.stage.pickHint")}</span>
            </div>
            <div className="px-3.5">
              <StackBar
                className="mt-2"
                ariaLabel={t("disk.stack.aria", {
                  l1: formatBytes(totals.deadweight),
                  l2: formatBytes(totals.l2),
                  l3: formatBytes(totals.l3),
                  l4: formatBytes(totals.l4),
                })}
                segments={[
                  { key: "l1", label: `L1 ${t("disk.tier.l1")}`, value: totals.deadweight, tone: "accent" },
                  { key: "l2", label: `L2 ${t("disk.tier.l2")}`, value: totals.l2, tone: "info" },
                  { key: "l3", label: `L3 ${t("disk.tier.l3")}`, value: totals.l3, tone: "neutral" },
                  { key: "l4", label: `L4 ${t("disk.tier.l4")}`, value: totals.l4, muted: true },
                ]}
              />
              <p className="mt-1.5 text-[9.5px] text-ink-faint">{t("disk.stack.note")}</p>
            </div>

            <div className="mt-1 pb-1">
              {TIERS.map((level) => (
                <TierRow
                  key={level}
                  level={level}
                  bytes={
                    level === "l1"
                      ? totals.deadweight
                      : level === "l2"
                        ? totals.l2
                        : level === "l3"
                          ? totals.l3
                          : totals.l4
                  }
                  members={groupsOfTier(level)}
                  picked={picked}
                  expanded={openTier === level}
                  onToggleExpand={() => setOpenTier((cur) => (cur === level ? null : level))}
                  onToggleGroup={toggleGroup}
                />
              ))}
            </div>
          </section>
        ) : null}

        {/* 扫描进度（轻量一行，不抢视线） */}
        {scanning ? (
          <div className="mt-2 text-[10px] text-ink-faint">
            {progress
              ? t("disk.progressLine", {
                  done: progress.done,
                  total: progress.total,
                  phase: progress.phase,
                  bytes: formatBytes(progress.bytesSoFar),
                })
              : t("common.progress.scanning")}
          </div>
        ) : null}

        {/* A1 杀手卡：跨平台二进制（唯一「零后悔」的量） */}
        {a1?.deadweight ? (
          <DeadweightCard group={a1} dw={a1.deadweight} />
        ) : (
          <section className="mt-3">
            <h2 className="text-[11.5px] font-medium text-ink-dim">{t("disk.deadweight.noneTitle")}</h2>
            <p className="mt-1 text-[10px] leading-relaxed text-ink-faint">
              {t("disk.deadweight.noneNote")}
            </p>
          </section>
        )}

        {/* 同一批依赖存了 N 份 */}
        {report.dup ? (
          <section className="mt-2.5 rounded-card border border-warn/25 bg-warn/[0.06] px-3.5 py-2.5">
            <div className="flex items-baseline justify-between gap-3">
              <h2 className="text-[11.5px] font-medium text-ink">
                {t("disk.dup.title", { n: report.dup.managers.length })}
              </h2>
              <span className="font-mono text-[11.5px] tabular-nums text-warn">
                {formatBytes(report.dup.totalBytes)}
              </span>
            </div>
            {/* advice 由 Rust 生成：P4 批次随 commands 一起本地化，这里先用后端串 */}
            <p className="mt-1 text-[10px] leading-relaxed text-ink-faint">{report.dup.advice}</p>
            <div className="mt-1.5 flex flex-wrap gap-1.5">
              {report.dup.managers.map(([name, bytes]) => (
                <span
                  key={name}
                  className="rounded-ctl bg-chip px-1.5 py-0.5 text-[10px] text-ink-dim"
                >
                  {name} <span className="font-mono tabular-nums">{formatBytes(bytes)}</span>
                </span>
              ))}
            </div>
          </section>
        ) : null}

        {/* F9 项目体检：把「N 个 node_modules 共 X GB」拆成按项目的账本 */}
        <ProjectHealthCard projects={report.projects ?? []} />

        <p className="mt-3 text-[9px] leading-relaxed text-ink-faint">
          {t("disk.drawer.recycleNote")}
        </p>
      </div>

      {/* ③ 底部操作栏：选中态是唯一的执行入口 */}
      {totals.reclaimable > 0 ? (
        <div className="flex shrink-0 items-center gap-3 border-t border-hairline bg-surface px-5 py-2.5">
          <span className="text-[11px] text-ink-dim">
            {pickedGroups.length === 0
              ? t("disk.bar.pickedNone")
              : t("disk.bar.picked", { n: pickedGroups.length, size: formatBytes(pickedBytes) })}
          </span>
          <div className="grow" />
          <Btn
            variant="primary"
            disabled={pickedGroups.length === 0}
            onClick={() => void openDrawer()}
          >
            {t("disk.bar.clean")}
          </Btn>
        </div>
      ) : null}

      <CleanDrawer
        open={drawer || previewLoading}
        loading={previewLoading}
        preview={preview}
        busy={busy}
        onClose={closeDrawer}
        onExecute={(items, confirmed) => {
          void execute(confirmed, items).then(() => setPicked(new Set()));
        }}
      />
    </div>
  );
}

/** 一个分组「实际能删多少」：有 deadweight 的只算死重，否则整组 */
function reclaimableOf(g: CacheGroup): number {
  return g.deadweight ? g.deadweight.bytes : g.totalBytes;
}

/* ==========================================================================
   卷卡
   ========================================================================== */

function VolumeCard({ volume, reclaimable }: { volume: VolumeInfo; reclaimable: number }) {
  const { t } = useI18n();
  const used = Math.max(0, volume.totalBytes - volume.availableBytes);
  const usedRatio = volume.totalBytes > 0 ? used / volume.totalBytes : 0;
  const reclaimOfFree = volume.userAvailableBytes > 0 ? reclaimable / volume.userAvailableBytes : 0;
  // 容量越满环越红（80% 预警），环色即语义
  const ringColor =
    usedRatio >= 0.9
      ? "var(--color-danger)"
      : usedRatio >= 0.8
        ? "var(--color-warn)"
        : "var(--color-domain-net)";

  return (
    <section className="mt-2.5 flex items-center gap-3 rounded-card border border-hairline-soft bg-surface-raised px-3.5 py-2">
      <RingGauge
        segments={[{ value: Math.max(0.04, usedRatio), color: ringColor }]}
        size={56}
        stroke={5}
        ariaLabel={t("disk.volume.aria", {
          pct: formatPercent(usedRatio),
          used: formatBytes(used),
          total: formatBytes(volume.totalBytes),
        })}
      >
        <span className="flex items-baseline font-mono text-[12px] leading-none tabular-nums text-ink">
          {Math.round(usedRatio * 100)}
          <span className="text-[7px] text-ink-faint">%</span>
        </span>
      </RingGauge>
      <div className="min-w-0 flex-1">
        <div className="flex items-baseline justify-between gap-3">
          <span className="text-[11px] font-medium text-ink-dim">{t("disk.volume.title")}</span>
          <span className="font-mono text-[10.5px] tabular-nums text-ink-dim">
            {t("disk.volume.used", { used: formatBytes(used), total: formatBytes(volume.totalBytes) })}
            <span className="ml-2 text-ink">
              {t("disk.volume.free", { free: formatBytes(volume.userAvailableBytes) })}
            </span>
          </span>
        </div>
        <p className="mt-0.5 text-[9.5px] text-ink-faint">
          {t("disk.volume.reclaimable", { size: formatBytes(reclaimable) })}
          {reclaimOfFree > 0
            ? t("disk.volume.reclaimOfFree", { pct: Math.round(reclaimOfFree * 100) })
            : ""}
        </p>
      </div>
    </section>
  );
}

/* ==========================================================================
   ① + ② 分级构成 → 分组预览
   ========================================================================== */

function TierRow({
  level,
  bytes,
  members,
  picked,
  expanded,
  onToggleExpand,
  onToggleGroup,
}: {
  level: Level;
  /** 本级可回收量（与顶部总量、构成条同源 = report.totals） */
  bytes: number;
  /** 本级下的分组（L1 = 带死重切片的组，见 groupsOfTier） */
  members: CacheGroup[];
  picked: Set<string>;
  expanded: boolean;
  onToggleExpand: () => void;
  onToggleGroup: (key: string) => void;
}) {
  const { t, locale } = useI18n();
  const meta = LEVEL_META[level];
  const hint = t(
    level === "l1"
      ? "disk.tier.l1Hint"
      : level === "l2"
        ? "disk.tier.l2Hint"
        : level === "l3"
          ? "disk.tier.l3Hint"
          : "disk.tier.l4Hint",
  );
  const colors: Record<Level, string> = {
    l1: "var(--color-accent)",
    l2: "var(--color-info)",
    l3: "var(--color-warn)",
    l4: "var(--color-danger)",
  };
  const actionable = members.filter((g) => g.cleanable && level !== "l4").length;
  const max = Math.max(bytes, 1);
  const pickedHere = members.filter((g) => picked.has(g.key)).length;

  return (
    <div className="border-b border-hairline-soft last:border-b-0">
      <button
        type="button"
        onClick={onToggleExpand}
        aria-expanded={expanded}
        title={hint}
        className="flex w-full items-center gap-2 rounded-ctl px-3.5 py-[6px] text-left transition-colors hover:bg-chip"
      >
        <i
          className="size-[7px] shrink-0 rounded-[2px]"
          style={{ background: colors[level], opacity: level === "l4" ? 0.45 : 1 }}
          aria-hidden
        />
        <span className="shrink-0 text-[11.5px] text-ink">
          <span className="mr-1 font-mono text-[10px] text-ink-faint">{level.toUpperCase()}</span>
          {t(meta.labelKey)}
        </span>
        <span className="min-w-0 flex-1 truncate text-[9.5px] text-ink-faint">
          {level === "l4"
            ? t("disk.stage.commandOnly")
            : actionable > 0
              ? t("disk.stage.cleanable", { n: actionable })
              : t("disk.stage.statsOnly")}
        </span>
        <span className="h-[3px] w-16 shrink-0 overflow-hidden rounded-full bg-chip">
          <i
            className="block h-full rounded-full"
            style={{
              width: `${Math.max(2, (bytes / max) * 100)}%`,
              background: colors[level],
              opacity: level === "l4" ? 0.45 : 0.85,
            }}
          />
        </span>
        <span className="w-14 shrink-0 text-right font-mono text-[11px] tabular-nums text-ink">
          {formatBytes(bytes)}
        </span>
        <span
          className={`w-9 shrink-0 text-right font-mono text-[9.5px] tabular-nums ${
            pickedHere > 0 ? "text-accent" : "text-transparent"
          }`}
          aria-hidden
        >
          {pickedHere > 0 ? `${pickedHere}/${members.length}` : "0"}
        </span>
        <ChevronRightIcon
          size={11}
          className={`shrink-0 text-ink-faint transition-transform ${expanded ? "rotate-90" : ""}`}
        />
      </button>

      {expanded ? (
        <div className="border-t border-hairline-soft bg-surface-sunk/40 px-3.5 py-1">
          {members.length === 0 ? (
            <p className="py-1.5 text-[10px] text-ink-faint">{t("disk.group.statsOnlyTip")}</p>
          ) : (
            members.map((g) => (
              <GroupPick
                key={g.key}
                group={g}
                selectable={g.cleanable && level !== "l4"}
                checked={picked.has(g.key)}
                onToggle={() => onToggleGroup(g.key)}
                locale={locale}
              />
            ))
          )}
        </div>
      ) : null}
    </div>
  );
}

/** 勾选框：原生 input 才能拿到 indeterminate 与键盘可达 */
function Check({
  checked,
  indeterminate = false,
  disabled = false,
  onChange,
  label,
}: {
  checked: boolean;
  indeterminate?: boolean;
  disabled?: boolean;
  onChange: () => void;
  label: string;
}) {
  const ref = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (ref.current) ref.current.indeterminate = indeterminate && !checked;
  }, [indeterminate, checked]);
  return (
    <input
      ref={ref}
      type="checkbox"
      checked={checked}
      disabled={disabled}
      onChange={onChange}
      aria-label={label}
      className="size-[13px] shrink-0 cursor-pointer rounded-[3px] accent-[var(--color-accent)] disabled:cursor-default disabled:opacity-25"
    />
  );
}

/** 一个可清理分组：勾选 + 名称 + 路径 + 体积；右键定位 */
function GroupPick({
  group,
  selectable,
  checked,
  onToggle,
  locale,
}: {
  group: CacheGroup;
  selectable: boolean;
  checked: boolean;
  onToggle: () => void;
  locale: I18n["locale"];
}) {
  const { t } = useI18n();
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const details = group.details ?? [];
  const rootIsDir = group.cleanable && group.level !== "l4";
  // 有死重切片的分组挂在这一级的「可删量」是切片，而不是整个缓存目录
  const shownBytes = group.deadweight ? group.deadweight.bytes : group.totalBytes;

  return (
    <div
      className="group/row flex items-center gap-2 rounded-ctl py-[3px] pr-1 transition-colors hover:bg-chip"
      title={t("disk.fileList.title", { path: group.root, tip: t("disk.pathMenu.rootTip") })}
      onContextMenu={(e) => {
        e.preventDefault();
        if (!rootIsDir) {
          // 聚合项没有单一路径 —— 伪造一个会让「在 Finder 中打开」打到 home
          toast(t("disk.pathMenu.aggregateTip"));
          return;
        }
        setMenu({ x: e.clientX, y: e.clientY });
      }}
    >
      <Check checked={checked} disabled={!selectable} onChange={onToggle} label={cacheLabel(group, locale)} />
      <span className="min-w-0 flex-1">
        <span className="block truncate text-[11px] text-ink">
          {cacheLabel(group, locale)}
          {group.deadweight ? (
            <span className="ml-1.5 text-[9px] text-accent/80">{t("disk.stage.slice")}</span>
          ) : null}
        </span>
        <span className="block truncate font-mono text-[9.5px] text-ink-faint">
          {group.entryCount > 0 ? `${t("disk.group.entryCount", { n: group.entryCount })} · ` : ""}
          {rootIsDir ? group.root : t("disk.group.statsOnlyTip")}
        </span>
      </span>
      {details.length > 0 ? <GroupDetails details={details} /> : null}
      <span className="w-14 shrink-0 text-right font-mono text-[11px] tabular-nums text-ink-dim">
        {formatBytes(shownBytes)}
      </span>
      {menu ? (
        <PathMenu x={menu.x} y={menu.y} path={group.root} isDir onClose={() => setMenu(null)} />
      ) : null}
    </div>
  );
}

/** A4/A5 明细：按体积降序（Rust 侧已排），Top 20 + 「N 天未改动」，>180 天标灰 */
function GroupDetails({ details }: { details: DetailItem[] }) {
  const { t } = useI18n();
  const stale = details.filter((d) => d.ageDays > STALE_DAYS).length;
  const shown = details.slice(0, 20);
  const [menu, setMenu] = useState<{ x: number; y: number; path: string } | null>(null);
  return (
    <details className="shrink-0">
      <summary className="cursor-pointer list-none text-[9.5px] text-ink-faint transition-colors hover:text-ink-dim">
        {t("disk.details.summary", { n: details.length })}
        {stale > 0 ? t("disk.details.stale", { n: stale, days: STALE_DAYS }) : ""}
      </summary>
      <div className="mt-1 space-y-0.5 rounded-ctl bg-surface-sunk px-2 py-1">
        {shown.map((d) => (
          <div
            key={d.path}
            className="flex items-baseline gap-2 text-[10px]"
            onContextMenu={(e) => {
              e.preventDefault();
              setMenu({ x: e.clientX, y: e.clientY, path: d.path });
            }}
            title={t("disk.fileList.title", { path: d.path, tip: t("disk.pathMenu.rootTip") })}
          >
            <span className="w-14 shrink-0 text-right font-mono tabular-nums text-ink-dim">
              {formatBytes(d.bytes)}
            </span>
            <span className="w-20 shrink-0 text-ink-faint">{formatAge(d.ageDays, t)}</span>
            <span
              className={`min-w-0 flex-1 truncate ${d.ageDays > STALE_DAYS ? "text-ink-faint" : "text-ink-dim"}`}
            >
              {d.note ?? d.name}
            </span>
          </div>
        ))}
        {details.length > shown.length ? (
          <p className="text-[9.5px] text-ink-faint">{t("disk.details.more", { n: details.length - shown.length })}</p>
        ) : null}
      </div>
      {menu ? (
        <PathMenu
          x={menu.x}
          y={menu.y}
          path={menu.path}
          isDir
          onClose={() => setMenu(null)}
        />
      ) : null}
    </details>
  );
}

/** A4 活跃度分界（与 prune-core STALE_DAYS 对齐）：超过即标灰 */
const STALE_DAYS = 180;

function formatAge(days: number, t: I18n["t"]): string {
  if (days <= 0) return t("disk.age.recent");
  if (days < 30) return t("common.unit.unchanged", { n: days });
  if (days < 365) return t("common.unit.unchangedMonths", { n: Math.round(days / 30) });
  return t("common.unit.unchangedYears", { n: (days / 365).toFixed(1) });
}

/* ==========================================================================
   ③ 清理抽屉
   ========================================================================== */

function CleanDrawer({
  open,
  loading,
  preview,
  busy,
  onClose,
  onExecute,
}: {
  open: boolean;
  loading: boolean;
  preview: ReturnType<typeof useDiskStore.getState>["preview"];
  busy: boolean;
  onClose: () => void;
  onExecute: (items: ReclaimItem[], confirmed: boolean) => void;
}) {
  const { t, locale } = useI18n();
  const report = useDiskStore((s) => s.report);
  const groups = report?.groups ?? [];
  /** 抽屉内逐条勾选：默认全选，取消的条目不会传给后端 */
  const [drop, setDrop] = useState<Set<string>>(new Set());
  const [confirmed, setConfirmed] = useState(false);
  /** 模态 a11y（§11.2）：焦点陷阱 + 关闭后焦点回到触发元素 */
  const dialogRef = useDialogA11y<HTMLDivElement>(open);

  // 每次拿到新清单都重置勾选与二次确认（旧状态套新清单 = 静默漏删/多删）
  const items = preview?.items;
  useEffect(() => {
    setDrop(new Set());
    setConfirmed(false);
  }, [items]);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      // 清理中禁止关闭：避免「以为取消了其实还在删」
      if (e.key === "Escape" && !busy) onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, busy, onClose]);

  if (!open) return null;
  // 清理成功后 store 会清空 preview —— 此时抽屉必须消失，不能停在骨架屏上
  if (!loading && !preview) return null;

  const list = (preview?.items ?? []).filter((it) => !drop.has(it.path));
  const size = list.reduce((n, it) => n + it.bytes, 0);
  const l4Count = list.filter((it) => it.level === "l4").length;
  const needsConfirm = !!preview?.needsConfirm || l4Count > 0;

  return (
    <div className="absolute inset-0 z-[82] flex flex-col justify-end">
      <div
        className="prune-fade-in absolute inset-0 bg-black/45 backdrop-blur-[2px]"
        onClick={() => (busy ? undefined : onClose())}
      />
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-label={t("disk.drawer.title")}
        className="prune-drawer-in relative flex max-h-[70%] flex-col rounded-t-2xl border-t border-hairline bg-surface-overlay shadow-[0_-18px_44px_rgba(0,0,0,0.45)]"
      >
        <div className="flex items-start gap-3 px-5 pt-3 pb-2">
          <div className="min-w-0 flex-1">
            <div className="flex items-baseline gap-2">
              <span className="text-[13px] font-semibold text-ink">{t("disk.drawer.title")}</span>
            </div>
            <p className="mt-0.5 text-[10px] text-ink-faint">
              {loading
                ? t("disk.preview.building")
                : t("disk.drawer.summary", { n: list.length, size: formatBytes(size) })}
              {" · "}
              {t("disk.drawer.sub")}
            </p>
          </div>
          <button
            type="button"
            onClick={onClose}
            disabled={busy}
            className="grid size-6 shrink-0 place-items-center rounded-ctl text-ink-faint transition-colors hover:bg-chip hover:text-ink disabled:opacity-40"
            title={t("common.action.cancel")}
          >
            ✕
          </button>
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto px-5">
          {loading || !preview ? (
            <div className="space-y-1.5 py-2">
              {[0, 1, 2, 3].map((i) => (
                <div key={i} className="flex items-center gap-2">
                  <span className="size-3 shrink-0 animate-pulse rounded-[3px] bg-chip" />
                  <span className="h-2.5 w-56 animate-pulse rounded bg-chip" />
                  <span className="ml-auto h-2.5 w-12 shrink-0 animate-pulse rounded bg-chip" />
                </div>
              ))}
            </div>
          ) : (
            <ul className="space-y-0.5 pb-1">
              {preview.items.map((it) => (
                <DrawerItem
                  key={it.path}
                  item={it}
                  checked={!drop.has(it.path)}
                  onToggle={() =>
                    setDrop((prev) => {
                      const next = new Set(prev);
                      if (next.has(it.path)) next.delete(it.path);
                      else next.add(it.path);
                      return next;
                    })
                  }
                />
              ))}
            </ul>
          )}
          {preview?.notes.map((n, i) => (
            <p key={`${n.code}-${i}`} className="mb-1 text-[9.5px] leading-snug text-warn/90">
              {reclaimNoteText(n, locale, groups)}
            </p>
          ))}
        </div>

        <div className="shrink-0 border-t border-hairline-soft px-5 py-2.5">
          {needsConfirm ? (
            <label className="mb-2 flex items-start gap-2 rounded-ctl border border-warn/25 bg-warn/[0.08] px-2.5 py-1.5">
              <input
                type="checkbox"
                checked={confirmed}
                onChange={(e) => setConfirmed(e.target.checked)}
                className="mt-[1px] size-3 shrink-0 accent-[var(--color-warn)]"
              />
              <span className="text-[9.5px] leading-snug text-warn">{t("disk.drawer.l4warn", { n: l4Count })}</span>
            </label>
          ) : null}
          <div className="flex items-center gap-2.5">
            <span className="min-w-0 flex-1 truncate text-[9.5px] text-ink-faint">
              {t("disk.drawer.recycleNote")}
            </span>
            <Btn onClick={onClose} disabled={busy}>
              {t("common.action.cancel")}
            </Btn>
            <Btn
              variant={needsConfirm ? "danger" : "primary"}
              loading={busy}
              disabled={list.length === 0 || (needsConfirm && !confirmed)}
              onClick={() => onExecute(list, confirmed)}
            >
              {busy
                ? t("disk.drawer.cleaning")
                : list.length === 0
                  ? t("disk.drawer.empty")
                  : t("disk.drawer.clean", { size: formatBytes(size) })}
            </Btn>
          </div>
        </div>
      </div>
    </div>
  );
}

/** 抽屉里的一行：勾选 + 名称 + 体积；可展开一级子项（按需加载，不预取 50 次 IPC） */
function DrawerItem({
  item,
  checked,
  onToggle,
}: {
  item: ReclaimItem;
  checked: boolean;
  onToggle: () => void;
}) {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);

  return (
    <li>
      <div
        className="flex items-center gap-2 rounded-ctl px-1 py-[3px] transition-colors hover:bg-chip"
        onContextMenu={(e) => {
          e.preventDefault();
          setMenu({ x: e.clientX, y: e.clientY });
        }}
      >
        <Check checked={checked} onChange={onToggle} label={item.label} />
        <button
          type="button"
          onClick={() => setOpen((v) => !v)}
          aria-expanded={open}
          className="flex min-w-0 flex-1 items-center gap-1.5 text-left"
          title={t("disk.drawer.itemTip")}
        >
          <ChevronRightIcon
            size={10}
            className={`shrink-0 text-ink-faint transition-transform ${open ? "rotate-90" : ""}`}
          />
          <span className={`min-w-0 flex-1 truncate text-[10.5px] ${checked ? "text-ink-dim" : "text-ink-faint line-through"}`}>
            {item.label}
          </span>
        </button>
        <span className="w-14 shrink-0 text-right font-mono text-[10.5px] tabular-nums text-ink-dim">
          {formatBytes(item.bytes)}
        </span>
      </div>
      {open ? (
        <div className="ml-[21px] pb-1">
          <FileList root={item.path} />
        </div>
      ) : null}
      {menu ? (
        <PathMenu x={menu.x} y={menu.y} path={item.path} isDir onClose={() => setMenu(null)} />
      ) : null}
    </li>
  );
}

/* ==========================================================================
   路径右键菜单（spec §7.4）—— 统一走 PopMenu，命名空间与结束进程菜单隔离
   ========================================================================== */

function PathMenu({
  x,
  y,
  path,
  isDir,
  onClose,
}: {
  x: number;
  y: number;
  path: string;
  isDir: boolean;
  onClose: () => void;
}) {
  const { t } = useI18n();

  const reveal = async () => {
    if (!isTauri()) {
      toast(t("disk.err.needsDesktop"), "error");
      return;
    }
    try {
      await revealPath(path);
      toast(t("disk.pathMenu.revealOk"), "ok");
    } catch (e) {
      toast(String(e), "error");
    }
  };

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(path);
      toast(t("disk.pathMenu.copyOk"), "ok");
    } catch {
      toast(t("disk.pathMenu.copyFail"), "error");
    }
  };

  return (
    <PopMenu
      x={x}
      y={y}
      onClose={onClose}
      items={[
        {
          label: isDir ? t("disk.pathMenu.openDir") : t("disk.pathMenu.revealFile"),
          icon: isDir ? <FolderIcon size={12} /> : <FileIcon size={12} />,
          onSelect: () => void reveal(),
        },
        { label: t("disk.pathMenu.copyPath"), icon: <span className="font-mono text-[10px]">⧉</span>, onSelect: () => void copy() },
      ]}
    />
  );
}

/**
 * 一级子项列表：像 Finder 一样逐条列出文件/目录。
 * 只在用户点开某条时挂载 —— 旧版对清单里每条都无条件拉一次 `reclaim_children`，
 * 50 条清单就是 50 次 du，抽屉一开就卡。
 */
function FileList({ root }: { root: string }) {
  const [children, setChildren] = useState<ReclaimChild[] | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [menu, setMenu] = useState<{ x: number; y: number; path: string; isDir: boolean } | null>(null);
  const { t } = useI18n();

  useEffect(() => {
    let alive = true;
    if (!isTauri()) {
      // 浏览器 dev 模式：演示数据
      setChildren([
        { name: "content-v2", bytes: 7.6 * 1024 ** 3, isDir: true },
        { name: "index-v5", bytes: 0.4 * 1024 ** 3, isDir: true },
        { name: "tmp", bytes: 0, isDir: true },
      ]);
      return;
    }
    reclaimChildren(root)
      .then((c) => {
        if (alive) setChildren(c);
      })
      .catch((e) => {
        if (alive) setErr(String(e));
      });
    return () => {
      alive = false;
    };
  }, [root]);

  return (
    <>
      <ul className="mt-0.5 max-h-32 overflow-y-auto rounded-ctl bg-surface-sunk py-0.5">
        {children === null ? (
          err ? (
            <li className="px-2 py-1 text-[9.5px] text-danger">{err}</li>
          ) : (
            [0, 1, 2].map((i) => (
              <li key={i} className="flex items-center gap-2 px-2 py-[3px]">
                <span className="size-3 shrink-0 animate-pulse rounded-sm bg-chip" />
                <span className="h-2 w-40 animate-pulse rounded bg-chip" />
                <span className="ml-auto h-2 w-10 shrink-0 animate-pulse rounded bg-chip" />
              </li>
            ))
          )
        ) : (
          children.map((c) => {
            const full = joinPath(root, c.name);
            return (
              <li key={c.name}>
                <button
                  type="button"
                  onClick={() =>
                    void (isTauri()
                      ? revealPath(full).catch((e) => toast(String(e), "error"))
                      : toast(t("disk.err.needsDesktop"), "error"))
                  }
                  onContextMenu={(e) => {
                    e.preventDefault();
                    setMenu({ x: e.clientX, y: e.clientY, path: full, isDir: c.isDir ?? false });
                  }}
                  className="flex w-full items-center gap-2 px-2 py-[2px] text-left text-[9.5px] text-ink-dim transition-colors hover:bg-chip hover:text-ink"
                >
                  {c.isDir ? (
                    <FolderIcon size={10} className="shrink-0 text-accent/70" />
                  ) : (
                    <FileIcon size={10} className="shrink-0 text-ink-faint" />
                  )}
                  <span className="min-w-0 flex-1 truncate font-mono">{c.name}</span>
                  <span className="shrink-0 font-mono tabular-nums text-ink-faint">
                    {c.bytes > 0 ? formatBytes(c.bytes) : "—"}
                  </span>
                </button>
              </li>
            );
          })
        )}
      </ul>
      {menu ? (
        <PathMenu
          x={menu.x}
          y={menu.y}
          path={menu.path}
          isDir={menu.isDir}
          onClose={() => setMenu(null)}
        />
      ) : null}
    </>
  );
}

function joinPath(root: string, name: string): string {
  return `${root.replace(/\/+$/, "")}/${name}`;
}

/* ==========================================================================
   A1 杀手卡：跨平台二进制
   ========================================================================== */

function DeadweightCard({ group, dw }: { group: CacheGroup; dw: DeadweightStats }) {
  const loadPreview = useDiskStore((s) => s.loadPreview);
  const busy = useDiskStore((s) => s.busy);
  const { t, locale } = useI18n();

  return (
    <section className="mt-2.5 rounded-card border border-accent/25 bg-accent/[0.06] px-3.5 py-2.5">
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <h2 className="text-[12px] font-semibold tracking-tight text-ink">
            🟢 {t("disk.deadweight.title", { size: formatBytes(dw.bytes) })}
          </h2>
          <p className="mt-0.5 text-[9.5px] text-ink-faint">
            {t("disk.deadweight.meta", {
              label: cacheLabel(group, locale),
              n: dw.entryCount,
            })}
            {dw.ratio != null ? t("disk.deadweight.ratio", { ratio: dw.ratio.toFixed(1) }) : ""}
            {t("disk.deadweight.native", { size: formatBytes(dw.nativeBytes) })}
          </p>
        </div>
        <Btn variant="primary" onClick={() => void loadPreview([group.key])} disabled={busy}>
          {t("disk.deadweight.preview")}
        </Btn>
      </div>

      <div className="mt-1.5 grid grid-cols-2 gap-x-6">
        {dw.byPlatform.map((p) => (
          <div key={p.platform} className="flex items-baseline justify-between text-[9.5px]">
            <span className="font-mono text-ink-dim">{p.platform}</span>
            <span className="font-mono tabular-nums text-ink-faint">
              {t("disk.deadweight.platformEntry", { n: p.entryCount, size: formatBytes(p.bytes) })}
            </span>
          </div>
        ))}
      </div>

      {/* 浪费 vs 可用双段条：「浪费比 3.2 : 1」的比例一眼可见，数字仍保留 */}
      <StackBar
        className="mt-2"
        height={4}
        ariaLabel={
          dw.ratio != null
            ? t("disk.deadweight.ariaRatio", {
                waste: formatBytes(dw.bytes),
                native: formatBytes(dw.nativeBytes),
                ratio: dw.ratio.toFixed(1),
              })
            : t("disk.deadweight.aria", {
                waste: formatBytes(dw.bytes),
                native: formatBytes(dw.nativeBytes),
              })
        }
        segments={[
          { key: "waste", label: t("disk.deadweight.segWaste"), value: dw.bytes, tone: "accent" },
          { key: "native", label: t("disk.deadweight.segNative"), value: dw.nativeBytes, tone: "neutral" },
        ]}
      />
      <p className="mt-1 text-[9.5px] text-ink-faint">
        {dw.ratio != null
          ? t("disk.deadweight.summaryRatio", {
              waste: formatBytes(dw.bytes),
              native: formatBytes(dw.nativeBytes),
              ratio: dw.ratio.toFixed(1),
            })
          : t("disk.deadweight.summary", {
              waste: formatBytes(dw.bytes),
              native: formatBytes(dw.nativeBytes),
            })}
      </p>

      {dw.byPlatform.length > 1 ? (
        <BarList
          className="mt-1.5"
          rows={dw.byPlatform.map((p) => ({
            key: p.platform,
            label: p.platform,
            value: p.bytes,
            display: formatBytes(p.bytes),
            title: t("disk.deadweight.barTitle", {
              platform: p.platform,
              n: p.entryCount,
              size: formatBytes(p.bytes),
            }),
          }))}
        />
      ) : null}

      <details className="mt-1.5">
        <summary className="cursor-pointer text-[9.5px] text-ink-faint hover:text-ink-dim">
          {t("disk.deadweight.topItems", { n: Math.min(dw.items.length, 10) })}
        </summary>
        <ul className="mt-1 space-y-0.5">
          {dw.items.slice(0, 10).map((it) => (
            <li key={it.path} className="flex items-baseline gap-2 text-[9.5px]">
              <span className="w-14 shrink-0 text-right font-mono tabular-nums text-ink-dim">
                {formatBytes(it.bytes)}
              </span>
              <span className="w-16 shrink-0 text-ink-faint">{it.platform ?? "—"}</span>
              <span className="min-w-0 flex-1 truncate text-ink-dim" title={it.name}>
                {it.name}
              </span>
            </li>
          ))}
        </ul>
      </details>

      {dw.otherBytes > 0 ? (
        <p className="mt-1.5 text-[9.5px] text-ink-faint">
          {t("disk.deadweight.other", { size: formatBytes(dw.otherBytes) })}
        </p>
      ) : null}
    </section>
  );
}
