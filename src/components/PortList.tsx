import { useEffect, useMemo, useRef, useState } from "react";

import { ChevronRightIcon } from "./icons";
import { Badge } from "./primitives";
import { StackBar } from "./charts/StackBar";
import { DotMatrix } from "./charts/DotMatrix";
import { EmptyState } from "./EmptyArt";
import { LanBoard } from "./LanBoard";
import { formatClock } from "../lib/format";
import { useI18n } from "../i18n/store";
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
  const { t } = useI18n();
  const snapshot = usePortStore((s) => s.snapshot);
  const loading = usePortStore((s) => s.loading);
  const error = usePortStore((s) => s.error);
  const showFolded = usePortStore((s) => s.showFolded);
  const toggleFolded = usePortStore((s) => s.toggleFolded);
  const focusQuery = usePortStore((s) => s.focusQuery);
  const clearFocus = usePortStore((s) => s.clearFocus);
  const [query, setQuery] = useState("");
  // M18-C5：端口 / 局域网 分段（spec §11——进分段不扫描）
  const [page, setPage] = useState<PortSubPage>("ports");
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
      <div className="flex items-center justify-between gap-4">
        <div className="min-w-0">
          <h1 className="text-[15px] font-semibold tracking-tight">{t("ports.title")}</h1>
          <p className="mt-0.5 text-[11px] text-ink-faint">
            {page === "ports" ? t("ports.subPorts") : t("ports.subLan")}
          </p>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          {page === "ports" ? (
            <input
              ref={searchRef}
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder={t("ports.searchPlaceholder")}
              spellCheck={false}
              className="w-44 rounded-md border border-hairline bg-white/[0.04] px-2.5 py-1.5 text-[11px] text-ink placeholder:text-ink-faint focus:border-accent/50 focus:outline-none"
            />
          ) : null}
          <PortSubTabs page={page} onChange={setPage} />
        </div>
      </div>

      {page === "lan" ? (
        <LanBoard />
      ) : (
      <>
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
          {t("ports.invisibleLead", {
            total: snapshot?.coverage.listenTotal ?? 0,
            invisible,
          })}{" "}
          <code className="font-mono text-[10.5px]">sudo lsof -nP -iTCP -sTCP:LISTEN</code>
          {t("ports.invisibleTail")}
        </p>
      ) : null}

      {/* TIME_WAIT：会占端口但没有任何进程持有——不渲染成「被占用」，也不给结束入口 */}
      {snapshot?.timeWait ? (
        <p className="mt-3 rounded-lg bg-white/[0.03] px-3 py-2 text-[11px] leading-relaxed text-ink-dim">
          {t("ports.timeWait", {
            n: snapshot.timeWait,
            ports:
              snapshot.timeWaitPorts.length > 0
                ? t("ports.timeWaitPorts", {
                    ports:
                      snapshot.timeWaitPorts.slice(0, 8).join(" / ") +
                      (snapshot.timeWaitPorts.length > 8 ? " …" : ""),
                  })
                : "",
          })}
        </p>
      ) : null}

      {loading && !snapshot ? (
        <p className="mt-4 text-[11px] text-ink-faint">{t("ports.scanning")}</p>
      ) : !snapshot ? (
        <p className="mt-4 text-[11px] text-ink-faint">{t("ports.emptyNeverScanned")}</p>
      ) : filtered.length === 0 ? (
        entries.length === 0 ? (
          <EmptyState
            title={t("ports.emptyCleanTitle")}
            note={t("ports.emptyCleanNote")}
          />
        ) : (
          <p className="mt-4 text-[11px] text-ink-faint">
            {t("ports.noMatch", { query })}
          </p>
        )
      ) : (
        <>
          {/* UI v4（§6.6）点阵仪表盘：一端口一点，琥珀=对外 / 绿=关注层 / 灰=其他，
              hover 出「端口 · 进程」；占比堆叠条保留作分母视角 */}
          {allEntries.length > 0 ? (
            <div className="mt-3">
              <div className="flex items-baseline justify-between">
                <span className="text-[11px] font-medium text-ink-dim">{t("ports.board.title")}</span>
                <span className="text-[10px] text-ink-faint">
                  {t("ports.board.summary", {
                    watched: watchedOnly,
                    exposed: exposedOnly,
                    other: otherN,
                  })}
                </span>
              </div>
              <DotMatrix
                className="mt-2"
                ariaLabel={t("ports.board.dotAria", {
                  watched: watchedOnly,
                  exposed: exposedOnly,
                  other: otherN,
                })}
                items={allEntries.map((e) => {
                  const p = primaryOf(e);
                  return {
                    tone: e.exposure === "any" ? "warn" : e.watched ? "accent" : "neutral",
                    title: `${e.port} · ${p ? p.comm : t("ports.board.kernelOwned")}`,
                  };
                })}
              />
              <StackBar
                className="mt-2.5"
                ariaLabel={t("ports.board.barAria", {
                  watched: watchedOnly,
                  exposed: exposedOnly,
                  other: otherN,
                })}
                segments={[
                  { key: "w", label: t("ports.board.segWatched"), value: watchedOnly, tone: "accent" },
                  { key: "e", label: t("ports.board.segExposed"), value: exposedOnly, tone: "warn" },
                  { key: "o", label: t("ports.board.segOther"), value: otherN, tone: "neutral" },
                ]}
              />
            </div>
          ) : null}
          <div className="mt-3 grid grid-cols-[74px_78px_minmax(0,1fr)_minmax(0,1.05fr)_54px] items-center gap-2.5 border-b border-hairline-soft px-2 pb-2 text-[10px] text-ink-faint">
            <span>{t("ports.col.port")}</span>
            <span>{t("ports.col.exposure")}</span>
            <span>{t("ports.col.process")}</span>
            <span>{t("ports.col.project")}</span>
            <span className="text-right">{t("ports.col.conns")}</span>
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
                {t("ports.folded", { n: folded.length })}
                <span className="text-[10px] text-ink-faint">{t("ports.foldedNote")}</span>
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
            {t("ports.footer", {
              ports: snapshot.entries.length,
              watched: snapshot.entries.filter((e) => e.watched).length,
              ms: snapshot.scanMs,
              clock: formatClock(snapshot.at),
            })}
          </p>
        </>
      )}
      </>
      )}
    </div>
  );
}

/** 端口 / 局域网 两分段（spec §11：局域网挂 ports tab 内，不新增一级导航） */
type PortSubPage = "ports" | "lan";

function PortSubTabs({
  page,
  onChange,
}: {
  page: PortSubPage;
  onChange: (p: PortSubPage) => void;
}) {
  const { t } = useI18n();
  const items: { key: PortSubPage; label: string }[] = [
    { key: "ports", label: t("ports.subpage.ports") },
    { key: "lan", label: t("ports.subpage.lan") },
  ];
  return (
    <div
      className="flex shrink-0 rounded-full bg-white/[0.05] p-0.5"
      role="tablist"
      aria-label={t("ports.subpage.aria")}
    >
      {items.map((it) => {
        const active = page === it.key;
        return (
          <button
            key={it.key}
            type="button"
            role="tab"
            aria-selected={active}
            onClick={() => onChange(it.key)}
            className={`rounded-full px-3 py-1 text-[11px] transition-colors ${
              active
                ? "bg-white/[0.08] font-medium text-ink"
                : "text-ink-faint hover:text-ink-dim"
            }`}
          >
            {it.label}
          </button>
        );
      })}
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
  const { t, locale } = useI18n();
  const primary = primaryOf(entry);
  const owners = ownersOf(entry);
  const exposure = exposureMeta(entry.exposure, locale);
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
        title={killable ? t("ports.row.hintKillable") : t("ports.row.hintLocked")}
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
            {primary ? primary.comm : t("ports.row.kernelOwned")}
            {primary?.pid != null ? (
              <span className="ml-1.5 font-mono text-[10px] text-ink-faint">{primary.pid}</span>
            ) : null}
          </span>
          <span className="mt-0.5 flex items-center gap-1.5 text-[10px] text-ink-faint">
            {owners.length > 1 ? <span>{t("ports.row.owners", { n: owners.length })}</span> : null}
            {dual ? <span>IPv4+IPv6</span> : null}
            {system ? <span className="text-ink-faint">{t("ports.row.system")}</span> : null}
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
