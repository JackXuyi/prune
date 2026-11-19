import { useI18n } from "../../i18n/store";
import { formatClock } from "../../lib/format";
import type { PortSnapshot } from "../../types/net";
import { DotMatrix } from "../charts/DotMatrix";
import { RefreshIcon } from "../icons";

/**
 * 端口 Hero（eul 重设计 · ports-eul-mockup.html ②）：
 * 点阵图是主角（一端口一点），点色 琥珀=对外暴露 / 绿=关注 / 灰=其他。
 * 右侧是扫描口径块——「重新扫描」从隐形快捷键变成显性按钮（本页唯一执行入口）。
 */
export function PortsHero({
  snapshot,
  refreshing,
  onRescan,
}: {
  snapshot: PortSnapshot;
  refreshing: boolean;
  onRescan: () => void;
}) {
  const { t } = useI18n();
  const entries = snapshot.entries;
  const exposed = entries.filter((e) => e.exposure === "any").length;
  const watched = entries.filter((e) => e.watched).length;
  const other = Math.max(0, entries.length - exposed - watched);

  const dots = entries.map((e) => {
    const primary = e.listeners.find((b) => b.pid != null) ?? null;
    return {
      tone: (e.exposure === "any" ? "warn" : e.watched ? "accent" : "neutral") as
        | "warn"
        | "accent"
        | "neutral",
      title: `${e.port} · ${primary ? primary.comm : t("ports.row.kernelOwned")}`,
    };
  });

  return (
    <section
      className="relative flex h-[128px] shrink-0 items-center gap-4 overflow-hidden rounded-[16px] border px-[18px] py-3"
      style={{
        borderColor: "color-mix(in srgb, var(--color-domain-port) 26%, transparent)",
        backgroundImage:
          "linear-gradient(120deg, color-mix(in srgb, var(--color-domain-port) 9%, transparent), color-mix(in srgb, var(--color-domain-port) 2%, transparent) 46%, transparent 76%)",
        backgroundColor: "var(--color-surface-raised)",
      }}
      title={t("ports.hero.tip")}
    >
      <div className="min-w-0 flex-1">
        <div className="flex items-baseline gap-2.5">
          <span className="font-mono text-[25px] leading-none font-semibold tabular-nums text-ink">
            {entries.length}
          </span>
          <span className="min-w-0 truncate text-[10px] text-ink-faint">
            {t("ports.hero.leadNote")}
          </span>
        </div>
        <DotMatrix
          className="mt-2.5"
          dot={7}
          gap={5}
          max={48}
          ariaLabel={t("ports.hero.dotAria", { exposed, watched, other })}
          items={dots}
        />
        <div className="mt-2 flex flex-wrap gap-x-3 gap-y-1">
          <LegendDot swatch="var(--color-warn)" label={t("ports.hero.legendExposed")} value={exposed} />
          <LegendDot swatch="var(--color-accent)" label={t("ports.hero.legendWatched")} value={watched} />
          <LegendDot swatch="var(--color-hatch)" label={t("ports.hero.legendOther")} value={other} />
        </div>
      </div>

      {/* 扫描口径块：何时 / 耗时 / 归属 —— 重扫是显性入口（原 ⌘R 只在快捷键里） */}
      <div className="flex w-[168px] shrink-0 flex-col gap-1.5">
        <div className="flex items-baseline justify-between text-[9.5px] text-ink-faint">
          <span>{t("ports.hero.scanAt")}</span>
          <b className="font-mono text-[10.5px] font-medium tabular-nums text-ink-dim">
            {formatClock(snapshot.at)}
          </b>
        </div>
        <div className="flex items-baseline justify-between text-[9.5px] text-ink-faint">
          <span>{t("ports.hero.scanMs")}</span>
          <b className="font-mono text-[10.5px] font-medium tabular-nums text-ink-dim">
            {snapshot.scanMs} ms
          </b>
        </div>
        <div className="flex items-baseline justify-between text-[9.5px] text-ink-faint">
          <span>{t("ports.hero.attribution")}</span>
          <b className="font-mono text-[10.5px] font-medium tabular-nums text-ink-dim">
            {snapshot.coverage.attributed} / {snapshot.coverage.listenTotal}
          </b>
        </div>
        <button
          type="button"
          onClick={onRescan}
          disabled={refreshing}
          className="mt-0.5 flex h-[26px] items-center justify-center gap-1.5 rounded-[8px] border border-hairline bg-chip text-[11px] text-ink-dim transition-colors hover:text-ink disabled:opacity-40"
        >
          <RefreshIcon size={11} className={refreshing ? "animate-spin" : ""} />
          {refreshing ? t("common.progress.scanning") : t("ports.hero.rescan")}
        </button>
      </div>
    </section>
  );
}

function LegendDot({
  swatch,
  label,
  value,
}: {
  swatch: string;
  label: string;
  value: number;
}) {
  return (
    <span className="flex items-center gap-1.5 text-[9.5px] text-ink-faint">
      <em className="size-[7px] shrink-0 rounded-[2px]" style={{ background: swatch }} aria-hidden />
      {label}
      <b className="font-mono text-[10.5px] font-medium tabular-nums text-ink-dim">{value}</b>
    </span>
  );
}
