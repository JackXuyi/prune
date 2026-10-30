import { useEffect, useState } from "react";

import { formatBytes, formatRelative } from "../lib/format";
import { isTauri, reclaimChildren, revealPath } from "../lib/ipc";
import { useDiskStore } from "../stores/diskStore";
import { LEVEL_META, type CacheGroup, type DeadweightStats, type DetailItem, type Level, type ReclaimChild, type VolumeInfo } from "../types/disk";
import { FileIcon, FolderIcon, RefreshIcon } from "./icons";
import { Badge, Meter } from "./primitives";

/** M4/S2：存储 tab 顶部的卷容量条（已用/总量 + 可回收相当于剩余空间的比例） */
function VolumeBar({
  volume,
  reclaimable,
}: {
  volume: VolumeInfo;
  reclaimable: number;
}) {
  const used = Math.max(0, volume.totalBytes - volume.availableBytes);
  const usedRatio = volume.totalBytes > 0 ? used / volume.totalBytes : 0;
  const reclaimOfFree =
    volume.userAvailableBytes > 0 ? reclaimable / volume.userAvailableBytes : 0;

  return (
    <section className="mt-4 border-b border-hairline-soft pb-4">
      <div className="flex items-baseline justify-between">
        <span className="text-[11px] font-medium text-ink-dim">卷容量</span>
        <span className="font-mono text-[11px] tabular-nums text-ink-dim">
          已用 {formatBytes(used)} / {formatBytes(volume.totalBytes)}
          <span className="ml-2 text-ink">剩余 {formatBytes(volume.userAvailableBytes)}</span>
        </span>
      </div>
      <Meter ratio={usedRatio} tone="neutral" height={5} className="mt-2" />
      <p className="mt-1.5 text-[10px] text-ink-faint">
        可回收 <span className="font-mono tabular-nums text-accent">{formatBytes(reclaimable)}</span>
        {reclaimOfFree > 0
          ? ` · 相当于剩余空间的 ${Math.round(reclaimOfFree * 100)}%`
          : ""}
      </p>
    </section>
  );
}

/** A4 活跃度分界（与 prune-core STALE_DAYS 对齐）：超过即标灰 */
const STALE_DAYS = 180;

function formatAge(days: number): string {
  if (days <= 0) return "最近有改动";
  if (days < 30) return `${days} 天未改动`;
  if (days < 365) return `${Math.round(days / 30)} 个月未改动`;
  return `${(days / 365).toFixed(1)} 年未改动`;
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
        <h1 className="text-[15px] font-semibold tracking-tight">可回收空间</h1>
        <p className="mt-1 text-[11px] text-ink-faint">
          {scanning
            ? `正在扫描…${progress ? `（${progress.done}/${progress.total}）` : ""}`
            : "首次扫描在启动 30 秒后自动开始，也可以手动触发。"}
        </p>
        <button
          type="button"
          onClick={() => void refresh()}
          disabled={scanning}
          className="mt-3 flex items-center gap-1.5 rounded-md bg-white/[0.05] px-2.5 py-1.5 text-[11px] text-ink-dim transition-colors hover:bg-white/[0.09] hover:text-ink disabled:opacity-50"
        >
          <RefreshIcon size={12} className={scanning ? "animate-spin" : ""} />
          {scanning ? "扫描中" : "立即扫描"}
        </button>
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
            <span className="ml-1.5 text-[13px] font-normal text-ink-faint">可回收</span>
          </h1>
          <p className="mt-1 text-[11px] text-ink-faint">
            L1 零后悔 + L2 可再生 + L3 需判断 · Docker 等 L4 高风险项不计入
          </p>
        </div>
        <button
          type="button"
          onClick={() => void refresh()}
          disabled={scanning}
          className="mt-1 flex shrink-0 items-center gap-1.5 rounded-md bg-white/[0.05] px-2.5 py-1.5 text-[11px] text-ink-dim transition-colors hover:bg-white/[0.09] hover:text-ink disabled:opacity-50"
        >
          <RefreshIcon size={12} className={scanning ? "animate-spin" : ""} />
          {scanning ? "扫描中" : "重新扫描"}
        </button>
      </div>

      {/* M4/S2 存储域分母：可回收的 GB 从此有上下文 */}
      {report.volume ? <VolumeBar volume={report.volume} reclaimable={report.totals.reclaimable} /> : null}

      <LayerBar totals={report.totals} />

      {scanning ? (
        <div className="mt-3 text-[10px] text-ink-faint">
          {progress
            ? `扫描中 ${progress.done}/${progress.total} · ${progress.phase} · 已统计 ${formatBytes(progress.bytesSoFar)}`
            : "扫描中…"}
        </div>
      ) : null}

      {lastReclaimed ? (
        <div className="mt-3 flex items-center justify-between rounded-lg bg-accent/[0.08] px-3 py-2 text-[11px] text-accent">
          <span>
            已移入废纸篓 {lastReclaimed.moved} 项 · {formatBytes(lastReclaimed.bytes)}
            {lastReclaimed.skipped > 0 ? ` · 跳过 ${lastReclaimed.skipped} 个已消失条目` : ""}
            {lastReclaimed.failed.length > 0 ? ` · 失败 ${lastReclaimed.failed.length} 项` : ""}
          </span>
          <button type="button" onClick={clearReclaimed} className="text-[10px] text-ink-dim hover:text-ink">
            知道了
          </button>
        </div>
      ) : null}

      {a1?.deadweight ? (
        <DeadweightCard group={a1} dw={a1.deadweight} />
      ) : (
        <section className="mt-4">
          <h2 className="text-[12px] font-medium text-ink-dim">跨平台二进制 · 未发现</h2>
          <p className="mt-1 text-[10.5px] leading-relaxed text-ink-faint">
            Yarn v6 缓存不存在，或其中只有本机架构的包——本地单架构安装时这是正常状态。
          </p>
        </section>
      )}

      {report.dup ? (
        <section className="mt-4 rounded-xl bg-warn/[0.06] p-3.5">
          <div className="flex items-baseline justify-between">
            <h2 className="text-[12px] font-medium text-ink">
              同一批依赖存了 {report.dup.managers.length} 份
            </h2>
            <span className="font-mono text-[12px] tabular-nums text-warn">
              {formatBytes(report.dup.totalBytes)}
            </span>
          </div>
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

      <section className="mt-4">
        <div className="flex items-baseline justify-between">
          <h2 className="text-[12px] font-medium text-ink-dim">全部可回收项</h2>
          <span className="text-[10px] text-ink-faint">
            {report.groups.length} 组 · 扫描 {formatRelative(report.scannedAt)} · 耗时{" "}
            {(report.scanElapsedMs / 1000).toFixed(1)} s
            {report.incremental ? " · 增量" : ""}
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

function LayerBar({ totals }: { totals: { deadweight: number; l2: number; l3: number; l4: number } }) {
  const rows: { level: Level; tag: string; label: string; bytes: number; hint: string }[] = [
    { level: "l1", tag: "L1", label: "跨平台二进制", bytes: totals.deadweight, hint: "零后悔 · 你的 Mac 永远加载不了" },
    { level: "l2", tag: "L2", label: "包管理器等缓存", bytes: totals.l2, hint: "可再生 · 删了会重建，只慢下一次安装" },
    { level: "l3", tag: "L3", label: "项目依赖 / 模拟器", bytes: totals.l3, hint: "需判断 · 要重装或丢模拟器状态，逐个确认" },
    { level: "l4", tag: "L4", label: "Docker 等高风险项", bytes: totals.l4, hint: "只提供命令，不执行，不计入主数字" },
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

  return (
    <section className="mt-4 rounded-xl bg-accent/[0.06] p-3.5">
      <div className="flex items-start justify-between gap-3">
        <div>
          <h2 className="text-[13px] font-semibold tracking-tight text-ink">
            🟢 {formatBytes(dw.bytes)} · 你的 Mac 永远加载不了
          </h2>
          <p className="mt-0.5 text-[10.5px] text-ink-faint">
            {group.label} · {dw.entryCount} 个跨平台二进制缓存包
            {dw.ratio != null ? ` · 浪费比 ${dw.ratio.toFixed(1)} : 1` : ""} · 本机可用{" "}
            {formatBytes(dw.nativeBytes)}
          </p>
        </div>
        <button
          type="button"
          onClick={() => void loadPreview([group.key])}
          disabled={busy}
          className="shrink-0 rounded-md bg-accent/[0.14] px-2.5 py-1.5 text-[11px] font-medium text-accent transition-colors hover:bg-accent/25 disabled:opacity-50"
        >
          预览清理
        </button>
      </div>

      <div className="mt-2.5 grid grid-cols-2 gap-x-6 gap-y-1">
        {dw.byPlatform.map((p) => (
          <div key={p.platform} className="flex items-baseline justify-between text-[10.5px]">
            <span className="text-ink-dim">{p.platform}</span>
            <span className="text-ink-faint">
              {p.entryCount} 个 · <span className="font-mono tabular-nums text-ink-dim">{formatBytes(p.bytes)}</span>
            </span>
          </div>
        ))}
      </div>

      <details className="mt-2">
        <summary className="cursor-pointer text-[10.5px] text-ink-faint hover:text-ink-dim">
          最大的 {Math.min(dw.items.length, 10)} 个条目
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
          另有 {formatBytes(dw.otherBytes)} 为普通包或依赖 Rosetta 的 darwin-x64 条目，不在默认清理清单内。
        </p>
      ) : null}
    </section>
  );
}

function GroupRow({ group }: { group: CacheGroup }) {
  const loadPreview = useDiskStore((s) => s.loadPreview);
  const busy = useDiskStore((s) => s.busy);
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
      title={rootIsDir ? `${group.root}\n右键可打开所在目录` : group.root}
    >
      <div className="flex items-center gap-2">
        <span className="min-w-0 flex-1">
          <span className="block truncate text-[11.5px] text-ink">{group.label}</span>
          <span className="mt-0.5 block truncate text-[10px] text-ink-faint" title={group.root}>
            {group.entryCount > 0 ? `${group.entryCount} 项 · ` : ""}
            {group.root}
          </span>
        </span>
        <Badge tone={meta.tone}>{meta.label}</Badge>
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
            预览
          </button>
        ) : (
          <span
            className="w-16 shrink-0 text-center text-[10px] text-ink-faint"
            title={group.level === "l4" ? "只提供命令，不执行" : "分散在多个项目目录，只做统计"}
          >
            {group.level === "l4" ? "手动处理" : "只统计"}
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
  const stale = details.filter((d) => d.ageDays > STALE_DAYS).length;
  const shown = details.slice(0, 20);
  const [menu, setMenu] = useState<PathTarget | null>(null);
  return (
    <details className="mt-1 pl-2">
      <summary className="cursor-pointer text-[10px] text-ink-faint hover:text-ink-dim">
        {details.length} 项明细（按体积）
        {stale > 0 ? ` · ${stale} 项超过 ${STALE_DAYS} 天未改动` : ""}
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
            title={`${d.path}\n右键可打开所在目录`}
          >
            <span className="w-14 shrink-0 text-right font-mono tabular-nums text-ink-dim">
              {formatBytes(d.bytes)}
            </span>
            <span className="w-20 shrink-0 text-ink-faint">{formatAge(d.ageDays)}</span>
            <span
              className={`min-w-0 flex-1 truncate ${d.ageDays > STALE_DAYS ? "text-ink-faint" : "text-ink-dim"}`}
            >
              {d.note ?? d.name}
            </span>
          </li>
        ))}
        {details.length > shown.length ? (
          <li className="text-[10px] text-ink-faint">… 另有 {details.length - shown.length} 项</li>
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
      setErr("浏览器预览模式无法打开 Finder");
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
      setErr("复制失败");
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
          {target.isDir ? "在 Finder 中打开" : "在 Finder 中显示"}
        </button>
        <button
          type="button"
          onClick={() => void copy()}
          className="flex w-full items-center gap-2 px-2.5 py-1.5 text-left text-[11px] text-ink-dim transition-colors hover:bg-white/[0.06] hover:text-ink"
        >
          <span className="w-3 text-center font-mono text-[10px] text-ink-faint">⧉</span>
          {copied ? "已复制" : "复制路径"}
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
                  title={`${full}\n单击在 Finder 中打开 · 右键更多`}
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

  // 弹层先出：清单还没生成时只渲染骨架
  if (!preview) {
    return (
      <div className="fixed inset-x-0 bottom-0 z-20 border-t border-hairline bg-[#0b0e11]/95 px-5 py-3 backdrop-blur-xl">
        <h3 className="text-[12px] font-medium text-ink">
          正在生成清理清单<span className="animate-pulse text-ink-faint">…</span>
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
            待清理 <span className="font-mono tabular-nums text-accent">{formatBytes(preview.totalBytes)}</span>
            <span className="ml-1.5 text-[10px] font-normal text-ink-faint">
              {preview.items.length} 项 · 全部走废纸篓，可恢复
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
                  title={`${it.path}\n右键可打开所在目录`}
                >
                  <span className="w-14 shrink-0 text-right font-mono tabular-nums text-ink-dim">
                    {formatBytes(it.bytes)}
                  </span>
                  <span className="min-w-0 flex-1 truncate">{it.label}</span>
                </div>
                <div className="ml-[68px]">
                  <p className="text-[9.5px] text-ink-faint/80">一级条目 · 单击在 Finder 中打开，右键更多</p>
                  <FileList root={it.path} />
                </div>
              </li>
            ))}
            {preview.items.length > shown.length ? (
              <li className="text-[10px] text-ink-faint">… 另有 {preview.items.length - shown.length} 项</li>
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
              {preview.totalBytes > 20 * 1024 ** 3 ? "超过 20 GB，确认执行" : "该级别需确认后执行"}
            </label>
          ) : null}
          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={clear}
              className="rounded-md bg-white/[0.05] px-2.5 py-1.5 text-[11px] text-ink-dim transition-colors hover:bg-white/[0.09] hover:text-ink"
            >
              取消
            </button>
            <button
              type="button"
              disabled={busy || blocked || preview.items.length === 0}
              onClick={() => void execute(confirmed)}
              className="rounded-md bg-accent/[0.14] px-2.5 py-1.5 text-[11px] font-medium text-accent transition-colors hover:bg-accent/25 disabled:opacity-40"
            >
              {busy ? "执行中…" : "移入废纸篓"}
            </button>
          </div>
        </div>
      </div>
      {menu ? <PathMenu target={menu} onClose={() => setMenu(null)} /> : null}
    </div>
  );
}
