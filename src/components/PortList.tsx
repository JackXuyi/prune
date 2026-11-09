import { useEffect, useMemo, useRef, useState } from "react";

import { ChevronRightIcon } from "./icons";
import { Badge } from "./primitives";
import { StackBar } from "./charts/StackBar";
import { DotMatrix } from "./charts/DotMatrix";
import { EmptyState } from "./EmptyArt";
import { formatClock } from "../lib/format";
import { usePortStore } from "../stores/portStore";
import {
  canKillOwner,
  dualStack,
  exposureMeta,
  invisibleCount,
  isSystemProcess,
  localUrl,
  ownersOf,
  primaryOf,
  splitLayers,
  type PortBinding,
  type PortEntry,
} from "../types/net";

/** 端口 tab 的整块内容（主窗口第 4 个 tab） */
export function PortBoard({
  onOpenProcess,
  onKill,
}: {
  onOpenProcess: (pid: number) => void;
  onKill: (binding: PortBinding, port: number, x: number, y: number) => void;
}) {
  const snapshot = usePortStore((s) => s.snapshot);
  const loading = usePortStore((s) => s.loading);
  const error = usePortStore((s) => s.error);
  const showFolded = usePortStore((s) => s.showFolded);
  const toggleFolded = usePortStore((s) => s.toggleFolded);
  const focusQuery = usePortStore((s) => s.focusQuery);
  const clearFocus = usePortStore((s) => s.clearFocus);
  const [query, setQuery] = useState("");
  // ⌘F 聚焦搜索框（UX v3 §5.5）：主窗口全局派发 prune:focus-search
  const searchRef = useRef<HTMLInputElement>(null);
  useEffect(() => {
    const onFocus = () => searchRef.current?.focus();
    window.addEventListener("prune:focus-search", onFocus);
    return () => window.removeEventListener("prune:focus-search", onFocus);
  }, []);

  // 从应用行点端口徽标跳进来：预填搜索词并展开（否则命中的行可能还在折叠区里）
  useEffect(() => {
    if (!focusQuery) return;
    setQuery(focusQuery);
    clearFocus();
  }, [focusQuery, clearFocus]);

  // 搜索：端口号 / 进程名 / 项目名 / pid 都能搜——用户手里只有其中一种线索
  const entries = snapshot?.entries ?? [];
  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return entries;
    return entries.filter(
      (e) =>
        String(e.port).includes(q) ||
        e.listeners.some(
          (b) =>
            b.comm.toLowerCase().includes(q) ||
            (b.projectLabel ?? "").toLowerCase().includes(q) ||
            (b.project ?? "").toLowerCase().includes(q) ||
            String(b.pid ?? "").includes(q),
        ),
    );
  }, [entries, query]);

  const { watched, folded } = splitLayers(filtered);
  // 搜索时跨层：用户手里有明确线索（端口号/进程名）时，折叠区里的命中也必须出现
  const searching = query.trim() !== "";
  const visible = searching ? filtered : watched;
  const invisible = snapshot ? invisibleCount(snapshot.coverage) : 0;

  // 分组占比（§7.6）：三段互斥——「对外暴露」可能同时命中关注层，先扣掉再算其他，
  // 否则三段之和会大于端口总数、条形比例虚增。口径与总览端口卡一致。
  const allEntries = snapshot?.entries ?? [];
  const watchedOnly = allEntries.filter((e) => e.watched && e.exposure !== "any").length;
  const exposedOnly = allEntries.filter((e) => e.exposure === "any").length;
  const otherN = Math.max(0, allEntries.length - watchedOnly - exposedOnly);

  return (
    <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4">
      <div className="flex items-baseline justify-between gap-4">
        <div className="min-w-0">
          <h1 className="text-[15px] font-semibold tracking-tight">端口占用</h1>
          <p className="mt-0.5 text-[11px] text-ink-faint">
            按端口反查持有进程 · 关注层是开发服务与数据库端口 · 内部 IPC 默认折叠
          </p>
        </div>
        <input
          ref={searchRef}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="端口 / 进程 / 项目"
          spellCheck={false}
          className="w-52 shrink-0 rounded-md border border-hairline bg-white/[0.04] px-2.5 py-1.5 text-[11px] text-ink placeholder:text-ink-faint focus:border-accent/50 focus:outline-none"
        />
      </div>

      {error ? (
        <p className="mt-3 rounded-lg bg-danger/[0.08] px-3 py-2 text-[11px] text-danger">{error}</p>
      ) : null}

      {/* 覆盖面受限必须说出来：非 root 的 lsof 对别的用户的进程静默跳过 */}
      {snapshot?.coverage.note ? (
        <p className="mt-3 rounded-lg bg-warn/[0.07] px-3 py-2 text-[11px] leading-relaxed text-warn">
          {snapshot.coverage.note}
        </p>
      ) : null}
      {invisible > 0 ? (
        <p className="mt-3 rounded-lg bg-warn/[0.07] px-3 py-2 text-[11px] leading-relaxed text-warn">
          内核报告 {snapshot?.coverage.listenTotal} 个监听，其中 {invisible} 个无法归属到进程
          （没有权限时 lsof 会跳过别的用户的进程）。Prune 不会提权；需要时请手动执行{" "}
          <code className="font-mono text-[10.5px]">sudo lsof -nP -iTCP -sTCP:LISTEN</code>。
        </p>
      ) : null}

      {/* TIME_WAIT：会占端口但没有任何进程持有——不渲染成「被占用」，也不给结束入口 */}
      {snapshot?.timeWait ? (
        <p className="mt-3 rounded-lg bg-white/[0.03] px-3 py-2 text-[11px] leading-relaxed text-ink-dim">
          另有{" "}
          <span className="font-mono tabular-nums text-ink">{snapshot.timeWait}</span>{" "}
          个 TIME_WAIT 连接
          {snapshot.timeWaitPorts.length > 0 ? (
            <>
              （涉及端口{" "}
              <span className="font-mono text-[10.5px] text-ink">
                {snapshot.timeWaitPorts.slice(0, 8).join(" / ")}
                {snapshot.timeWaitPorts.length > 8 ? " …" : ""}
              </span>
              ）
            </>
          ) : null}
          。它们在内核保留期内仍会让新服务拿到 EADDRINUSE，但没有任何进程持有，约 2 分钟后自动消失——
          这不是漏查。
        </p>
      ) : null}

      {loading && !snapshot ? (
        <p className="mt-4 text-[11px] text-ink-faint">正在扫描监听端口…</p>
      ) : !snapshot ? (
        <p className="mt-4 text-[11px] text-ink-faint">
          还没有扫描结果。点右上角「刷新」扫一轮——这不代表没有端口占用。
        </p>
      ) : filtered.length === 0 ? (
        entries.length === 0 ? (
          <EmptyState
            title="很干净，没有监听端口 ✂️"
            note="当前用户可见范围内没有进程监听端口。扫描过了才是这个结论——不是没扫。"
          />
        ) : (
          <p className="mt-4 text-[11px] text-ink-faint">{`没有匹配「${query}」的端口。`}</p>
        )
      ) : (
        <>
          {/* UI v4（§6.6）点阵仪表盘：一端口一点，琥珀=对外 / 绿=关注层 / 灰=其他，
              hover 出「端口 · 进程」；占比堆叠条保留作分母视角 */}
          {allEntries.length > 0 ? (
            <div className="mt-3">
              <div className="flex items-baseline justify-between">
                <span className="text-[11px] font-medium text-ink-dim">端口仪表盘</span>
                <span className="text-[10px] text-ink-faint">
                  关注 {watchedOnly} · 对外 {exposedOnly} · 其他 {otherN}（已折叠）
                </span>
              </div>
              <DotMatrix
                className="mt-2"
                ariaLabel={`端口点阵：关注层 ${watchedOnly}，对外暴露 ${exposedOnly}，其他 ${otherN}`}
                items={allEntries.map((e) => {
                  const p = primaryOf(e);
                  return {
                    tone: e.exposure === "any" ? "warn" : e.watched ? "accent" : "neutral",
                    title: `${e.port} · ${p ? p.comm : "内核持有"}`,
                  };
                })}
              />
              <StackBar
                className="mt-2.5"
                ariaLabel={`端口分组：关注层 ${watchedOnly}，对外暴露 ${exposedOnly}，其他 ${otherN}`}
                segments={[
                  { key: "w", label: "关注层", value: watchedOnly, tone: "accent" },
                  { key: "e", label: "对外暴露", value: exposedOnly, tone: "warn" },
                  { key: "o", label: "其他", value: otherN, tone: "neutral" },
                ]}
              />
            </div>
          ) : null}
          <div className="mt-3 grid grid-cols-[74px_78px_minmax(0,1fr)_minmax(0,1.05fr)_54px] items-center gap-2.5 border-b border-hairline-soft px-2 pb-2 text-[10px] text-ink-faint">
            <span>端口</span>
            <span>暴露面</span>
            <span>进程</span>
            <span>项目</span>
            <span className="text-right">连接</span>
          </div>
          <ul>
            {visible.map((e) => (
              <PortRow
                key={`${e.port}-${e.proto}`}
                entry={e}
                onOpenProcess={onOpenProcess}
                onKill={onKill}
              />
            ))}
          </ul>

          {!searching && folded.length > 0 ? (
            <div className="mt-2 border-t border-hairline-soft pt-2">
              <button
                type="button"
                onClick={toggleFolded}
                className="flex w-full items-center gap-1.5 rounded-md px-2 py-1.5 text-left text-[11px] text-ink-dim transition-colors hover:bg-white/[0.04] hover:text-ink"
              >
                <ChevronRightIcon
                  size={11}
                  className={`shrink-0 text-ink-faint transition-transform ${showFolded ? "rotate-90" : ""}`}
                />
                其他 {folded.length} 个端口
                <span className="text-[10px] text-ink-faint">
                  · 应用内部 IPC / 系统服务，通常不是你的服务
                </span>
              </button>
              {showFolded ? (
                <ul>
                  {folded.map((e) => (
                    <PortRow
                      key={`${e.port}-${e.proto}`}
                      entry={e}
                      onOpenProcess={onOpenProcess}
                      onKill={onKill}
                    />
                  ))}
                </ul>
              ) : null}
            </div>
          ) : null}

          <p className="mt-3 text-[10px] text-ink-faint">
            {snapshot.entries.length} 个端口 · 关注 {snapshot.entries.filter((e) => e.watched).length}{" "}
            · 扫描 {snapshot.scanMs} ms · {formatClock(snapshot.at)}
          </p>
        </>
      )}
    </div>
  );
}

function PortRow({
  entry,
  onOpenProcess,
  onKill,
}: {
  entry: PortEntry;
  onOpenProcess: (pid: number) => void;
  onKill: (binding: PortBinding, port: number, x: number, y: number) => void;
}) {
  const primary = primaryOf(entry);
  const owners = ownersOf(entry);
  const exposure = exposureMeta(entry.exposure);
  const system = owners.length > 0 && owners.every((b) => isSystemProcess(b.comm));
  const killable = primary != null && canKillOwner(primary);
  const url = localUrl(entry.port);
  const dual = dualStack(entry);

  return (
    <li>
      <div
        onDoubleClick={() => {
          if (primary?.pid != null) onOpenProcess(primary.pid);
        }}
        onContextMenu={(e) => {
          if (!killable || !primary) return;
          e.preventDefault();
          onKill(primary, entry.port, e.clientX, e.clientY);
        }}
        title={
          killable
            ? "双击查看该进程明细 · 右键结束进程"
            : "该端口由系统进程或内核持有，不提供结束入口"
        }
        className="grid grid-cols-[74px_78px_minmax(0,1fr)_minmax(0,1.05fr)_54px] items-center gap-2.5 rounded-md px-2 py-1.5 transition-colors hover:bg-white/[0.04]"
      >
        <span className="flex items-baseline gap-1">
          <span className="font-mono text-[12.5px] leading-tight tabular-nums text-ink">
            {entry.port}
          </span>
          <span className="text-[9px] uppercase text-ink-faint">{entry.proto}</span>
        </span>

        <span>
          <Badge tone={exposure.tone} title={exposure.hint}>
            {exposure.label}
          </Badge>
        </span>

        <span className="min-w-0">
          <span className="block truncate text-[11.5px] text-ink">
            {primary ? primary.comm : "内核持有"}
            {primary?.pid != null ? (
              <span className="ml-1.5 font-mono text-[10px] text-ink-faint">{primary.pid}</span>
            ) : null}
          </span>
          <span className="mt-0.5 flex items-center gap-1.5 text-[10px] text-ink-faint">
            {owners.length > 1 ? <span>{owners.length} 个持有者</span> : null}
            {dual ? <span>IPv4+IPv6</span> : null}
            {system ? <span className="text-ink-faint">系统</span> : null}
          </span>
        </span>

        <span className="min-w-0">
          {primary?.projectLabel ? (
            <>
              <span className="block truncate text-[11.5px] text-ink-dim">
                {primary.projectLabel}
              </span>
              <span
                className="mt-0.5 block truncate font-mono text-[9.5px] text-ink-faint"
                title={primary.project ?? ""}
              >
                {primary.project}
              </span>
            </>
          ) : (
            <span className="text-[10.5px] text-ink-faint">—</span>
          )}
        </span>

        <span className="text-right font-mono text-[11px] tabular-nums text-ink-dim">
          {entry.conns == null ? "—" : entry.conns}
        </span>
      </div>

      {/* 内置解释（AirPlay 5000/7000）：命中才出现，不占位 */}
      {entry.note ? (
        <p className="mb-1 ml-2 pl-2 text-[10px] leading-snug text-warn">{entry.note}</p>
      ) : null}
      {/* 开发服务（有 cwd 归属）才给 loopback 地址：GUI 应用那行贴个 URL 只是噪音 */}
      {primary?.project && entry.proto === "tcp" ? (
        <p className="mb-1 ml-2 pl-2 font-mono text-[9.5px] text-ink-faint">{url}</p>
      ) : null}
    </li>
  );
}
