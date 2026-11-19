import { useEffect, useMemo, useRef, useState } from "react";

import { ChevronRightIcon } from "./icons";
import { Btn } from "./primitives";
import { EmptyState } from "./EmptyArt";
import { LanBoard } from "./LanBoard";
import { PORT_ROW_COLS, PortRow } from "./ports/PortRow";
import { PortsHero } from "./ports/PortsHero";
import { PortsRibbon } from "./ports/PortsRibbon";
import { formatClock } from "../lib/format";
import { useI18n } from "../i18n/store";
import { usePortStore } from "../stores/portStore";
import {
  invisibleCount,
  splitLayers,
  type PortBinding,
} from "../types/net";

/** 端口 tab 的整块内容（主窗口第 4 个 tab；双子页：端口 / 局域网） */
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
  const refreshing = usePortStore((s) => s.refreshing);
  const refresh = usePortStore((s) => s.refresh);
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

  // W2/D-09（§4.3）：用户在 prune.toml 里配置的关注端口，空闲也要显示——
  // 用户主动关注的要能看到状态变化（服务没起），而不是静默消失
  const configuredIdle = useMemo(() => {
    const configured = snapshot?.configuredWatchPorts ?? [];
    if (configured.length === 0) return [];
    const busy = new Set(entries.map((e) => e.port));
    return configured.filter((p) => !busy.has(p));
  }, [snapshot, entries]);

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-[9px] overflow-x-hidden overflow-y-auto px-4 pb-3.5 pt-3">
      {/* ⓪ 子页头：分段胶囊在左、搜索在右——两个子页共用，点胶囊切换主区 */}
      <div className="flex shrink-0 items-center gap-2.5">
        <PortSubTabs page={page} onChange={setPage} />
        <input
          ref={searchRef}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder={
            page === "ports" ? t("ports.searchPlaceholder") : t("ports.searchLanPlaceholder")
          }
          spellCheck={false}
          className="ml-auto h-[27px] w-[200px] rounded-lg border border-hairline bg-surface-sunk px-2.5 text-[11px] text-ink placeholder:text-ink-faint focus:border-accent/50 focus:outline-none"
        />
      </div>

      {page === "lan" ? (
        <LanBoard query={query} />
      ) : (
        <>
          {error ? (
            <p className="shrink-0 rounded-lg bg-danger/[0.08] px-3 py-2 text-[11px] text-danger">
              {error}
            </p>
          ) : null}

          {loading && !snapshot ? (
            <p className="mt-4 text-[11px] text-ink-faint">{t("ports.scanning")}</p>
          ) : !snapshot ? (
            <p className="mt-4 text-[11px] text-ink-faint">{t("ports.emptyNeverScanned")}</p>
          ) : (
            <>
              {/* ① 状态带 + ② Hero：首屏读数区（一屏装下，不滚动；变长内容在下方整页滚动） */}
              <PortsRibbon snapshot={snapshot} />
              {entries.length > 0 ? (
                <PortsHero
                  snapshot={snapshot}
                  refreshing={refreshing}
                  onRescan={() => void refresh()}
                />
              ) : null}

              {/* 覆盖面受限必须说出来：非 root 的 lsof 对别的用户的进程静默跳过 */}
              {snapshot.coverage.note ? (
                <p className="shrink-0 rounded-[10px] bg-warn/[0.07] px-3 py-[7px] text-[10.5px] leading-relaxed text-warn">
                  {snapshot.coverage.note}
                </p>
              ) : null}
              {invisible > 0 ? (
                <p className="shrink-0 rounded-[10px] bg-warn/[0.07] px-3 py-[7px] text-[10.5px] leading-relaxed text-warn">
                  {t("ports.invisibleLead", {
                    total: snapshot.coverage.listenTotal,
                    invisible,
                  })}{" "}
                  <code className="font-mono text-[10px]">sudo lsof -nP -iTCP -sTCP:LISTEN</code>
                  {t("ports.invisibleTail")}
                </p>
              ) : null}
              {configuredIdle.length > 0 ? (
                <p
                  className="shrink-0 rounded-[10px] bg-accent/[0.07] px-3 py-[7px] text-[10.5px] leading-relaxed text-ink-dim"
                  aria-label={t("ports.configIdleAria", { n: configuredIdle.length })}
                >
                  {t("ports.configIdle", {
                    ports: configuredIdle.map((p) => `:${p}`).join(" "),
                  })}
                </p>
              ) : null}
              {/* TIME_WAIT：会占端口但没有任何进程持有——不渲染成「被占用」，也不给结束入口 */}
              {snapshot.timeWait ? (
                <p className="shrink-0 rounded-[10px] bg-chip px-3 py-[7px] text-[10.5px] leading-relaxed text-ink-dim">
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

              {filtered.length === 0 ? (
                entries.length === 0 ? (
                  <EmptyState
                    title={t("ports.emptyCleanTitle")}
                    description={t("ports.emptyCleanNote")}
                  />
                ) : (
                  <EmptyState
                    art="search"
                    title={t("ports.noMatchTitle", { query })}
                    description={t("ports.noMatchNote")}
                    action={
                      <Btn
                        variant="ghost"
                        onClick={() => {
                          setQuery("");
                          searchRef.current?.focus();
                        }}
                      >
                        {t("ports.clearSearch")}
                      </Btn>
                    }
                  />
                )
              ) : (
                <>
                  <div
                    className={`${PORT_ROW_COLS} shrink-0 border-b border-hairline-soft px-2 pb-1.5 pt-1 text-[9px] text-ink-faint`}
                  >
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
                        className="flex w-full items-center gap-1.5 rounded-md px-2 py-1.5 text-left text-[11px] text-ink-dim transition-colors hover:bg-chip hover:text-ink"
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

                  <p className="mt-2 px-2 text-[9px] text-ink-faint">
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
      className="flex shrink-0 rounded-full bg-chip p-0.5"
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
            className={`rounded-full px-3.5 py-1 text-[11px] transition-colors ${
              active
                ? "bg-port-soft font-medium text-domain-port"
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
