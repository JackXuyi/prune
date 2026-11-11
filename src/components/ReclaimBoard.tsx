import { useEffect, useState } from "react";

import { useI18n } from "../i18n/store";
import type { I18n } from "../i18n";
import { formatBytes, formatPercent, formatRelative } from "../lib/format";
import { isTauri, reclaimChildren, revealPath } from "../lib/ipc";
import { useDiskStore } from "../stores/diskStore";
import { StackBar } from "./charts/StackBar";
import { BarList } from "./charts/BarList";
import { RingGauge } from "./charts/RingGauge";
import { EmptyState } from "./EmptyArt";
import { ProjectHealthCard } from "./ProjectHealthCard";
import { cacheLabel, LEVEL_META, type CacheGroup, type DeadweightStats, type DetailItem, type Level, type ReclaimChild, type VolumeInfo } from "../types/disk";
import { FileIcon, FolderIcon, RefreshIcon } from "./icons";
import { Badge, Meter } from "./primitives";

/** M4/S2：存储 tab 顶部的卷容量环（UI v4 §6.4：细环 + 环心百分比，替代文字条） */
function VolumeBar({
  volume,
  reclaimable,
}: {
  volume: VolumeInfo;
  reclaimable: number;
}) {
  const { t } = useI18n();
  const used = Math.max(0, volume.totalBytes - volume.availableBytes);
  const usedRatio = volume.totalBytes > 0 ? used / volume.totalBytes : 0;
  const reclaimOfFree =
    volume.userAvailableBytes > 0 ? reclaimable / volume.userAvailableBytes : 0;
  // 容量越满环越红（80% 预警），环色即语义
  const ringColor =
    usedRatio >= 0.9 ? "var(--color-danger)" : usedRatio >= 0.8 ? "var(--color-warn)" : "var(--color-domain-net)";

  return (
    <section className="mt-4 flex items-center gap-4 border-b border-hairline-soft pb-4">
      <RingGauge
        segments={[{ value: Math.max(0.04, usedRatio), color: ringColor }]}
        size={72}
        stroke={6}
        ariaLabel={t("disk.volume.aria", {
          pct: formatPercent(usedRatio),
          used: formatBytes(used),
          total: formatBytes(volume.totalBytes),
        })}
      >
        <span className="font-mono text-[13px] tabular-nums text-ink">
          {Math.round(usedRatio * 100)}
          <span className="text-[8px] text-ink-faint">%</span>
        </span>
      </RingGauge>
      <div className="min-w-0 flex-1">
        <div className="flex items-baseline justify-between">
          <span className="text-[11px] font-medium text-ink-dim">{t("disk.volume.title")}</span>
          <span className="font-mono text-[11px] tabular-nums text-ink-dim">
            {t("disk.volume.used", { used: formatBytes(used), total: formatBytes(volume.totalBytes) })}
            <span className="ml-2 text-ink">
              {t("disk.volume.free", { free: formatBytes(volume.userAvailableBytes) })}
            </span>
          </span>
        </div>
        <p className="mt-1.5 text-[10px] text-ink-faint">
          {t("disk.volume.reclaimable", { size: formatBytes(reclaimable) })}
          {reclaimOfFree > 0
            ? t("disk.volume.reclaimOfFree", { pct: Math.round(reclaimOfFree * 100) })
            : ""}
        </p>
      </div>
    </section>
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

/** 主窗口「可回收」tab：A1 杀手卡片 + A2 分组 + A3 合并建议 + dry-run → 清理 */
export function ReclaimBoard() {
  const report = useDiskStore((s) => s.report);
  const scanning = useDiskStore((s) => s.scanning);
  const progress = useDiskStore((s) => s.progress);
  const error = useDiskStore((s) => s.error);
  const preview = useDiskStore((s) => s.preview);
  const previewLoading = useDiskStore((s) => s.previewLoading);
  const lastReclaimed = useDiskStore((s) => s.lastReclaimed);
  const refresh = useDiskStore((s) => s.refresh);
  const clearReclaimed = useDiskStore((s) => s.clearReclaimed);
  const i18n = useI18n();
  const { t } = i18n;

  if (error) {
    return (
      <div className="m-4 rounded-lg bg-danger/[0.08] px-3 py-2 text-[11px] text-danger">
        {error}
      </div>
    );
  }

  if (!report) {
    return (
      <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4">
        <h1 className="text-[15px] font-semibold tracking-tight">{t("disk.title")}</h1>
        <p className="mt-1 text-[11px] text-ink-faint">
          {scanning
            ? progress
              ? t("disk.scanningWith", { done: progress.done, total: progress.total })
              : t("disk.scanning")
            : t("common.hint.firstScan")}
        </p>
        <button
          type="button"
          onClick={() => void refresh()}
          disabled={scanning}
          className="mt-3 flex items-center gap-1.5 rounded-md bg-white/[0.05] px-2.5 py-1.5 text-[11px] text-ink-dim transition-colors hover:bg-white/[0.09] hover:text-ink disabled:opacity-50"
        >
          <RefreshIcon size={12} className={scanning ? "animate-spin" : ""} />
          {scanning ? t("common.progress.scanning") : t("common.action.scanNow")}
        </button>
        {/* UI v4（§6.7）：剪刀线稿空态——等待扫描也有情绪 */}
        {!scanning ? (
          <EmptyState title={t("disk.empty.noReportTitle")} note={t("disk.empty.noReportNote")} />
        ) : null}
      </div>
    );
  }

  const a1 = report.groups.find((g) => g.deadweight != null) ?? null;

  return (
    <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4">
      <div className="flex items-start justify-between gap-4">
        <div>
          <h1 className="text-[22px] font-semibold leading-tight tracking-tight">
            <span className="mr-1 text-accent">⚡</span>
            {formatBytes(report.totals.reclaimable)}
            <span className="ml-1.5 text-[13px] font-normal text-ink-faint">
              {t("disk.reclaimableSuffix")}
            </span>
          </h1>
          <p className="mt-1 text-[11px] text-ink-faint">{t("disk.subnet")}</p>
        </div>
        <button
          type="button"
          onClick={() => void refresh()}
          disabled={scanning}
          className="mt-1 flex shrink-0 items-center gap-1.5 rounded-md bg-white/[0.05] px-2.5 py-1.5 text-[11px] text-ink-dim transition-colors hover:bg-white/[0.09] hover:text-ink disabled:opacity-50"
        >
          <RefreshIcon size={12} className={scanning ? "animate-spin" : ""} />
          {scanning ? t("common.progress.scanning") : t("common.action.rescan")}
        </button>
      </div>

      {/* M4/S2 存储域分母：可回收的 GB 从此有上下文 */}
      {report.volume ? <VolumeBar volume={report.volume} reclaimable={report.totals.reclaimable} /> : null}

      {/* UI v4（§6.7）：扫完了但没有可回收项——「很干净」是正反馈不是缺数据 */}
      {report.totals.reclaimable === 0 ? (
        <EmptyState title={t("disk.empty.cleanTitle")} note={t("disk.empty.cleanNote")} />
      ) : null}

      <ReclaimStack totals={report.totals} />

      <LayerBar totals={report.totals} />

      {scanning ? (
        <div className="mt-3 text-[10px] text-ink-faint">
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

      {lastReclaimed ? (
        <div className="relative mt-3 flex items-center justify-between rounded-lg bg-accent/[0.08] px-3 py-2 text-[11px] text-accent">
          {/* UI v4（§6.4）清理完成飘字：+N GB 从右上浮起，尊重 reduced-motion */}
          {lastReclaimed.bytes > 0 ? (
            <span
              className="prune-float-up pointer-events-none absolute right-4 top-1 font-mono text-[13px] font-semibold tabular-nums"
              aria-hidden
            >
              +{formatBytes(lastReclaimed.bytes)}
            </span>
          ) : null}
          <span>
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
          <button type="button" onClick={clearReclaimed} className="text-[10px] text-ink-dim hover:text-ink">
            {t("disk.reclaimed.dismiss")}
          </button>
        </div>
      ) : null}

      {a1?.deadweight ? (
        <DeadweightCard group={a1} dw={a1.deadweight} />
      ) : (
        <section className="mt-4">
          <h2 className="text-[12px] font-medium text-ink-dim">{t("disk.deadweight.noneTitle")}</h2>
          <p className="mt-1 text-[10.5px] leading-relaxed text-ink-faint">
            {t("disk.deadweight.noneNote")}
          </p>
        </section>
      )}

      {report.dup ? (
        <section className="mt-4 rounded-xl bg-warn/[0.06] p-3.5">
          <div className="flex items-baseline justify-between">
            <h2 className="text-[12px] font-medium text-ink">
              {t("disk.dup.title", { n: report.dup.managers.length })}
            </h2>
            <span className="font-mono text-[12px] tabular-nums text-warn">
              {formatBytes(report.dup.totalBytes)}
            </span>
          </div>
          {/* advice 由 Rust 生成：P4 批次随 commands 一起本地化，这里先用后端串 */}
          <p className="mt-1 text-[10.5px] leading-relaxed text-ink-faint">{report.dup.advice}</p>
          <div className="mt-2 flex flex-wrap gap-1.5">
            {report.dup.managers.map(([name, bytes]) => (
              <span
                key={name}
                className="rounded bg-white/[0.05] px-1.5 py-0.5 text-[10px] text-ink-dim"
              >
                {name} <span className="font-mono tabular-nums">{formatBytes(bytes)}</span>
              </span>
            ))}
          </div>
        </section>
      ) : null}

      {/* F9 项目体检：把「N 个 node_modules 共 X GB」拆成按项目的账本 */}
      <ProjectHealthCard projects={report.projects ?? []} />

      <section className="mt-4">
        <div className="flex items-baseline justify-between">
          <h2 className="text-[12px] font-medium text-ink-dim">{t("disk.groups.title")}</h2>
          <span className="text-[10px] text-ink-faint">
            {t("disk.groups.meta", {
              n: report.groups.length,
              when: formatRelative(report.scannedAt, i18n.locale),
              secs: (report.scanElapsedMs / 1000).toFixed(1),
            })}
            {report.incremental ? t("disk.groups.incremental") : ""}
          </span>
        </div>
        <ul className="mt-1.5 space-y-1">
          {report.groups
            .filter((g) => g.deadweight == null)
            .map((g) => (
              <GroupRow key={g.key} group={g} />
            ))}
        </ul>
      </section>

      {preview || previewLoading ? <PreviewPanel /> : null}
    </div>
  );
}

/**
 * 全量构成条（UX v3.1 §7.6）：L1–L4 一条堆叠，谁是大头一眼看出。
 * 分母 = 各段之和（L4 计入总宽但为斜纹段），与 LayerBar 的逐行相对条互为补充；
 * 总量为 0 时 StackBar 自身不渲染（不画空轨道冒充数据）。
 */
function ReclaimStack({ totals }: { totals: { deadweight: number; l2: number; l3: number; l4: number } }) {
  const { t } = useI18n();
  return (
    <div className="mt-4">
      <div className="flex items-baseline justify-between">
        <span className="text-[11px] font-medium text-ink-dim">{t("disk.stack.title")}</span>
        <span className="text-[10px] text-ink-faint">{t("disk.stack.note")}</span>
      </div>
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
      <p className="mt-1.5 text-[10px] text-ink-faint">
        {t("disk.tier.total", {
          l1: formatBytes(totals.deadweight),
          l2: formatBytes(totals.l2),
          l3: formatBytes(totals.l3),
          l4: formatBytes(totals.l4),
        })}
      </p>
    </div>
  );
}

function LayerBar({ totals }: { totals: { deadweight: number; l2: number; l3: number; l4: number } }) {
  const { t } = useI18n();
  const rows: { level: Level; tag: string; label: string; bytes: number; hint: string }[] = [
    { level: "l1", tag: "L1", label: t("disk.tier.l1"), bytes: totals.deadweight, hint: t("disk.tier.l1Hint") },
    { level: "l2", tag: "L2", label: t("disk.tier.l2"), bytes: totals.l2, hint: t("disk.tier.l2Hint") },
    { level: "l3", tag: "L3", label: t("disk.tier.l3"), bytes: totals.l3, hint: t("disk.tier.l3Hint") },
    { level: "l4", tag: "L4", label: t("disk.tier.l4"), bytes: totals.l4, hint: t("disk.tier.l4Hint") },
  ];
  const max = Math.max(...rows.map((r) => r.bytes), 1);

  return (
    <div className="mt-4 space-y-2.5 border-b border-hairline-soft pb-4">
      {rows.map((r) => {
        const meta = LEVEL_META[r.level];
        return (
          <div key={r.level} className="flex items-center gap-3" title={r.hint}>
            <span className="w-28 shrink-0 truncate text-[11px] text-ink-dim">
              <span className="font-mono text-[10px] text-ink-faint">{r.tag}</span> · {r.label}
            </span>
            <Meter ratio={r.bytes / max} tone={meta.tone} height={4} className="min-w-0 flex-1" />
            <span className="w-16 shrink-0 text-right font-mono text-[11.5px] tabular-nums text-ink">
              {formatBytes(r.bytes)}
            </span>
          </div>
        );
      })}
    </div>
  );
}

function DeadweightCard({ group, dw }: { group: CacheGroup; dw: DeadweightStats }) {
  const loadPreview = useDiskStore((s) => s.loadPreview);
  const busy = useDiskStore((s) => s.busy);
  const { t, locale } = useI18n();

  return (
    <section className="mt-4 rounded-xl bg-accent/[0.06] p-3.5">
      <div className="flex items-start justify-between gap-3">
        <div>
          <h2 className="text-[13px] font-semibold tracking-tight text-ink">
            🟢 {t("disk.deadweight.title", { size: formatBytes(dw.bytes) })}
          </h2>
          <p className="mt-0.5 text-[10.5px] text-ink-faint">
            {t("disk.deadweight.meta", {
              label: cacheLabel(group, locale),
              n: dw.entryCount,
            })}
            {dw.ratio != null ? t("disk.deadweight.ratio", { ratio: dw.ratio.toFixed(1) }) : ""}
            {t("disk.deadweight.native", { size: formatBytes(dw.nativeBytes) })}
          </p>
        </div>
        <button
          type="button"
          onClick={() => void loadPreview([group.key])}
          disabled={busy}
          className="shrink-0 rounded-md bg-accent/[0.14] px-2.5 py-1.5 text-[11px] font-medium text-accent transition-colors hover:bg-accent/25 disabled:opacity-50"
        >
          {t("disk.deadweight.preview")}
        </button>
      </div>

      <div className="mt-2.5 grid grid-cols-2 gap-x-6 gap-y-1">
        {dw.byPlatform.map((p) => (
          <div key={p.platform} className="flex items-baseline justify-between text-[10.5px]">
            <span className="text-ink-dim">{p.platform}</span>
            <span className="text-ink-faint">
              <span className="font-mono tabular-nums text-ink-dim">
                {t("disk.deadweight.platformEntry", {
                  n: p.entryCount,
                  size: formatBytes(p.bytes),
                })}
              </span>
            </span>
          </div>
        ))}
      </div>

      {/* 浪费 vs 可用双段条（§7.6）：「浪费比 3.2 : 1」的比例一眼可见，数字仍保留 */}
      <StackBar
        className="mt-2.5"
        height={5}
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
          {
            key: "native",
            label: t("disk.deadweight.segNative"),
            value: dw.nativeBytes,
            tone: "neutral",
          },
        ]}
      />
      <p className="mt-1 text-[10px] text-ink-faint">
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

      {/* 各平台浪费分布（§7.6「多实体比大小」）：谁是大头一眼看出 */}
      {dw.byPlatform.length > 1 ? (
        <BarList
          className="mt-2"
          rows={dw.byPlatform.map((p) => ({
            key: p.platform,
            label: p.platform,
            value: p.bytes,
            display: t("disk.deadweight.platformEntry", {
              n: p.entryCount,
              size: formatBytes(p.bytes),
            }),
            title: t("disk.deadweight.barTitle", {
              platform: p.platform,
              n: p.entryCount,
              size: formatBytes(p.bytes),
            }),
          }))}
        />
      ) : null}

      <details className="mt-2">
        <summary className="cursor-pointer text-[10.5px] text-ink-faint hover:text-ink-dim">
          {t("disk.deadweight.topItems", { n: Math.min(dw.items.length, 10) })}
        </summary>
        <ul className="mt-1.5 space-y-0.5">
          {dw.items.slice(0, 10).map((it) => (
            <li key={it.path} className="flex items-baseline gap-2 text-[10.5px]">
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
        <p className="mt-2 text-[10px] text-ink-faint">
          {t("disk.deadweight.other", { size: formatBytes(dw.otherBytes) })}
        </p>
      ) : null}
    </section>
  );
}

function GroupRow({ group }: { group: CacheGroup }) {
  const loadPreview = useDiskStore((s) => s.loadPreview);
  const busy = useDiskStore((s) => s.busy);
  const { t, locale } = useI18n();
  const meta = LEVEL_META[group.level];
  const details = group.details ?? [];
  const [menu, setMenu] = useState<PathTarget | null>(null);
  // L4 只给命令；聚合项（root 是 home）整项不可清，但 A4/A5 有明细时逐项确认
  const cleanable = group.cleanable && group.level !== "l4";
  const perItem = !cleanable && details.length > 0 && group.level !== "l4";
  const actionable = cleanable || perItem;
  // 聚合项的 root 是 home，右键打开它没有意义——只对「整根可清」的分组开放
  const rootIsDir = group.cleanable && group.level !== "l4";

  return (
    <li
      className="rounded-md px-2 py-1.5 transition-colors hover:bg-white/[0.03]"
      onContextMenu={(e) => {
        if (!rootIsDir) return;
        e.preventDefault();
        setMenu({ path: group.root, isDir: true, x: e.clientX, y: e.clientY });
      }}
      title={
        rootIsDir ? t("disk.fileList.title", { path: group.root, tip: t("disk.pathMenu.rootTip") }) : group.root
      }
    >
      <div className="flex items-center gap-2">
        <span className="min-w-0 flex-1">
          <span className="block truncate text-[11.5px] text-ink">{cacheLabel(group, locale)}</span>
          <span className="mt-0.5 block truncate text-[10px] text-ink-faint" title={group.root}>
            {group.entryCount > 0
              ? `${t("disk.group.entryCount", { n: group.entryCount })} · `
              : ""}
            {group.root}
          </span>
        </span>
        <Badge tone={meta.tone}>
          {meta.dot} {t(meta.labelKey)}
        </Badge>
        <span className="w-16 shrink-0 text-right font-mono text-[11.5px] tabular-nums text-ink">
          {formatBytes(group.totalBytes)}
        </span>
        {actionable ? (
          <button
            type="button"
            onClick={() => void loadPreview([group.key])}
            disabled={busy || group.totalBytes === 0}
            className="w-16 shrink-0 rounded bg-white/[0.05] px-1.5 py-1 text-[10px] text-ink-dim transition-colors hover:bg-accent/[0.12] hover:text-accent disabled:opacity-40"
          >
            {t("disk.group.preview")}
          </button>
        ) : (
          <span
            className="w-16 shrink-0 text-center text-[10px] text-ink-faint"
            title={group.level === "l4" ? t("disk.group.manualTip") : t("disk.group.statsOnlyTip")}
          >
            {group.level === "l4" ? t("disk.group.manual") : t("disk.group.statsOnly")}
          </span>
        )}
      </div>
      {details.length > 0 ? <GroupDetails details={details} /> : null}
      {menu ? <PathMenu target={menu} onClose={() => setMenu(null)} /> : null}
    </li>
  );
}

/** A4/A5 明细：按体积降序（Rust 侧已排），Top 20 + 「N 天未改动」，>180 天标灰 */
function GroupDetails({ details }: { details: DetailItem[] }) {
  const { t } = useI18n();
  const stale = details.filter((d) => d.ageDays > STALE_DAYS).length;
  const shown = details.slice(0, 20);
  const [menu, setMenu] = useState<PathTarget | null>(null);
  return (
    <details className="mt-1 pl-2">
      <summary className="cursor-pointer text-[10px] text-ink-faint hover:text-ink-dim">
        {t("disk.details.summary", { n: details.length })}
        {stale > 0 ? t("disk.details.stale", { n: stale, days: STALE_DAYS }) : ""}
      </summary>
      <ul className="mt-1 space-y-0.5">
        {shown.map((d) => (
          <li
            key={d.path}
            className="flex items-baseline gap-2 text-[10.5px]"
            onContextMenu={(e) => {
              e.preventDefault();
              setMenu({ path: d.path, isDir: true, x: e.clientX, y: e.clientY });
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
          </li>
        ))}
        {details.length > shown.length ? (
          <li className="text-[10px] text-ink-faint">
            {t("disk.details.more", { n: details.length - shown.length })}
          </li>
        ) : null}
      </ul>
      {menu ? <PathMenu target={menu} onClose={() => setMenu(null)} /> : null}
    </details>
  );
}

/** 右键菜单目标：路径 + 是否目录 + 屏幕坐标 */
type PathTarget = { path: string; isDir: boolean; x: number; y: number };

function joinPath(root: string, name: string): string {
  return `${root.replace(/\/+$/, "")}/${name}`;
}

/**
 * 路径右键菜单：在 Finder 中打开 / 复制路径。
 * 目录直接开窗，文件用 `open -R` 定位选中（Rust 侧 reveal_path）。
 */
function PathMenu({ target, onClose }: { target: PathTarget; onClose: () => void }) {
  const [err, setErr] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const { t } = useI18n();

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  // 视口内收口，避免贴边被裁
  const x = Math.min(target.x, window.innerWidth - 196);
  const y = Math.min(target.y, window.innerHeight - 112);

  const reveal = async () => {
    if (!isTauri()) {
      setErr(t("disk.err.needsDesktop"));
      return;
    }
    try {
      await revealPath(target.path);
      onClose();
    } catch (e) {
      setErr(String(e));
    }
  };

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(target.path);
      setCopied(true);
      onClose();
    } catch {
      setErr(t("common.action.copyFailed"));
    }
  };

  return (
    <>
      <div
        className="fixed inset-0 z-40"
        onClick={onClose}
        onContextMenu={(e) => {
          e.preventDefault();
          onClose();
        }}
      />
      <div
        className="fixed z-50 w-[184px] rounded-lg border border-hairline bg-surface-raised py-1 shadow-2xl shadow-black/50"
        style={{ left: x, top: y }}
      >
        <button
          type="button"
          onClick={() => void reveal()}
          className="flex w-full items-center gap-2 px-2.5 py-1.5 text-left text-[11px] text-ink-dim transition-colors hover:bg-white/[0.06] hover:text-ink"
        >
          {target.isDir ? <FolderIcon size={12} /> : <FileIcon size={12} />}
          {target.isDir ? t("disk.pathMenu.openDir") : t("disk.pathMenu.revealFile")}
        </button>
        <button
          type="button"
          onClick={() => void copy()}
          className="flex w-full items-center gap-2 px-2.5 py-1.5 text-left text-[11px] text-ink-dim transition-colors hover:bg-white/[0.06] hover:text-ink"
        >
          <span className="w-3 text-center font-mono text-[10px] text-ink-faint">⧉</span>
          {copied ? t("common.action.copied") : t("disk.pathMenu.copyPath")}
        </button>
        {err ? (
          <p className="border-t border-hairline-soft px-2.5 pt-1.5 pb-1 text-[10px] leading-snug text-danger">
            {err}
          </p>
        ) : null}
      </div>
    </>
  );
}

/**
 * 一级子项列表：像 Finder 一样逐条列出文件/目录。
 * 明细由独立命令按需加载（du 慢，不拖住预览弹层）；左键直接打开，右键出菜单。
 */
function FileList({ root }: { root: string }) {
  const [children, setChildren] = useState<ReclaimChild[] | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [menu, setMenu] = useState<PathTarget | null>(null);
  const [toast, setToast] = useState<string | null>(null);
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

  useEffect(() => {
    if (!toast) return;
    const t = setTimeout(() => setToast(null), 2600);
    return () => clearTimeout(t);
  }, [toast]);

  const openInFinder = async (p: string) => {
    try {
      await revealPath(p);
    } catch (e) {
      setToast(String(e));
    }
  };

  return (
    <>
      <ul className="mt-1 max-h-40 overflow-y-auto rounded-md bg-white/[0.02] py-1">
        {children === null ? (
          err ? (
            <li className="px-2 py-1.5 text-[10px] text-danger">{err}</li>
          ) : (
            [0, 1, 2].map((i) => (
              <li key={i} className="flex items-center gap-2 px-2 py-[5px]">
                <span className="size-3 shrink-0 animate-pulse rounded-sm bg-white/[0.08]" />
                <span className="h-2 w-40 animate-pulse rounded bg-white/[0.06]" />
                <span className="ml-auto h-2 w-10 shrink-0 animate-pulse rounded bg-white/[0.05]" />
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
                  onClick={() => void openInFinder(full)}
                  onContextMenu={(e) => {
                    e.preventDefault();
                    setMenu({ path: full, isDir: c.isDir ?? false, x: e.clientX, y: e.clientY });
                  }}
                  className="flex w-full items-center gap-2 px-2 py-[3px] text-left text-[10px] text-ink-dim transition-colors hover:bg-white/[0.05] hover:text-ink"
                  title={t("disk.fileList.title", {
                    path: full,
                    tip: t("disk.pathMenu.openWithTip"),
                  })}
                >
                  {c.isDir ? (
                    <FolderIcon size={11} className="shrink-0 text-accent/70" />
                  ) : (
                    <FileIcon size={11} className="shrink-0 text-ink-faint" />
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
      {toast ? (
        <div className="fixed bottom-16 left-5 z-50 rounded-md border border-danger/30 bg-danger/[0.12] px-3 py-2 text-[10.5px] text-danger shadow-xl">
          {toast}
        </div>
      ) : null}
      {menu ? <PathMenu target={menu} onClose={() => setMenu(null)} /> : null}
    </>
  );
}

function PreviewPanel() {
  const preview = useDiskStore((s) => s.preview)!;
  const execute = useDiskStore((s) => s.execute);
  const clear = useDiskStore((s) => s.clearPreview);
  const busy = useDiskStore((s) => s.busy);
  const [confirmed, setConfirmed] = useState(false);
  const [menu, setMenu] = useState<PathTarget | null>(null);
  const { t } = useI18n();

  // 弹层先出：清单还没生成时只渲染骨架
  if (!preview) {
    return (
      <div className="fixed inset-x-0 bottom-0 z-20 border-t border-hairline bg-[#0b0e11]/95 px-5 py-3 backdrop-blur-xl">
        <h3 className="text-[12px] font-medium text-ink">
          {t("disk.preview.building")}
          <span className="animate-pulse text-ink-faint">…</span>
        </h3>
        <div className="mt-2 space-y-1.5">
          {[0, 1, 2].map((i) => (
            <div key={i} className="flex items-center gap-2">
              <span className="h-2.5 w-14 animate-pulse rounded bg-white/[0.06]" />
              <span className="h-2.5 w-64 animate-pulse rounded bg-white/[0.05]" />
            </div>
          ))}
        </div>
      </div>
    );
  }

  const blocked = preview.needsConfirm && !confirmed;
  const shown = preview.items.slice(0, 50);

  return (
    <div className="fixed inset-x-0 bottom-0 z-20 border-t border-hairline bg-[#0b0e11]/95 px-5 py-3 backdrop-blur-xl">
      <div className="flex items-start justify-between gap-4">
        <div className="min-w-0">
          <h3 className="text-[12px] font-medium text-ink">
            {t("disk.preview.title", { size: formatBytes(preview.totalBytes) })}
            <span className="ml-1.5 text-[10px] font-normal text-ink-faint">
              {t("disk.preview.meta", { n: preview.items.length })}
            </span>
          </h3>
          <ul className="mt-1 max-h-56 overflow-y-auto pr-2">
            {shown.map((it) => (
              <li key={it.path}>
                <div
                  className="flex items-baseline gap-2 text-[10px] text-ink-faint"
                  onContextMenu={(e) => {
                    e.preventDefault();
                    setMenu({ path: it.path, isDir: true, x: e.clientX, y: e.clientY });
                  }}
                  title={t("disk.fileList.title", {
                    path: it.path,
                    tip: t("disk.pathMenu.rootTip"),
                  })}
                >
                  <span className="w-14 shrink-0 text-right font-mono tabular-nums text-ink-dim">
                    {formatBytes(it.bytes)}
                  </span>
                  <span className="min-w-0 flex-1 truncate">{it.label}</span>
                </div>
                <div className="ml-[68px]">
                  <p className="text-[9.5px] text-ink-faint/80">{t("disk.preview.itemTip")}</p>
                  <FileList root={it.path} />
                </div>
              </li>
            ))}
            {preview.items.length > shown.length ? (
              <li className="text-[10px] text-ink-faint">
                {t("disk.preview.more", { n: preview.items.length - shown.length })}
              </li>
            ) : null}
          </ul>
          {preview.notes.map((n) => (
            <p key={n} className="mt-1 text-[10px] text-warn/90">
              {n}
            </p>
          ))}
        </div>

        <div className="flex shrink-0 flex-col items-end gap-2">
          {preview.needsConfirm ? (
            <label className="flex items-center gap-1.5 text-[10.5px] text-warn">
              <input
                type="checkbox"
                checked={confirmed}
                onChange={(e) => setConfirmed(e.target.checked)}
                className="size-3 accent-[#34d399]"
              />
              {preview.totalBytes > 20 * 1024 ** 3
                ? t("disk.preview.confirmBig")
                : t("disk.preview.confirmLevel")}
            </label>
          ) : null}
          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={clear}
              className="rounded-md bg-white/[0.05] px-2.5 py-1.5 text-[11px] text-ink-dim transition-colors hover:bg-white/[0.09] hover:text-ink"
            >
              {t("common.action.cancel")}
            </button>
            <button
              type="button"
              disabled={busy || blocked || preview.items.length === 0}
              onClick={() => void execute(confirmed)}
              className="rounded-md bg-accent/[0.14] px-2.5 py-1.5 text-[11px] font-medium text-accent transition-colors hover:bg-accent/25 disabled:opacity-40"
            >
              {busy ? t("common.action.executing") : t("disk.preview.execute")}
            </button>
          </div>
        </div>
      </div>
      {menu ? <PathMenu target={menu} onClose={() => setMenu(null)} /> : null}
    </div>
  );
}
