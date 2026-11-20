/**
 * 菜单栏面板首屏两块「图形主角」（menubar-panel-eul-mockup W2）。
 *
 * ① PanelRings：内存 / 存储 / CPU 三环。三者都是百分比读数，套环合法；
 *    环心放百分比（不是图标）——面板只有 110 px 一格，语义交给环下标签行，
 *    环心留给最需要「扫一眼」的数字。环色直接吃 `loadTone` 的 tone，
 *    不为面板另造配色（同一份 `Readout` → 同一套语义）。
 * ② PanelPortBand：端口域三段芯片带。端口不是百分比，所以不套环；
 *    用 domain-port 域色 + 三段计数表达「监听/关注/暴露」的面积对比。
 *
 * 诚实纪律（与 statsReadout 三条红线一致）：
 * - `ratio == null` 或 status 非 ok → 环心 `—` 且**不画弧**（绝不画 0 弧）；
 * - 端口未扫描 → 整块换成一行「未扫描」，不画 0 个芯片；
 * - 暴露 0 → 该段不渲染（0 不占位）；
 * - 细节（swap / 可用 / load 三元）全部进 tooltip，环下只留一行主读数。
 */
import type { ReactNode } from "react";

import { useI18n } from "../../i18n/store";
import { portCounts, type Readout, type Tone } from "../../lib/statsReadout";
import { portConflicts, type PortSnapshot } from "../../types/net";

/** 环几何：半径 17.5 → 周长 109.96，dash 按占比裁 */
const R = 17.5;
const C = 2 * Math.PI * R;

const toneColor: Record<Tone, string> = {
  accent: "var(--color-accent)",
  warn: "var(--color-warn)",
  danger: "var(--color-danger)",
  info: "var(--color-info)",
  neutral: "var(--color-ink-faint)",
};

/** tooltip 全量口径：占比 + 全部副读数（面板只显示第一条，其余藏这里） */
function fullText(readout: Readout): string {
  const parts = [
    readout.status === "ok" && readout.ratio != null
      ? `${Math.round(readout.ratio * 100)}%`
      : null,
    ...readout.sub,
    readout.tag?.text ?? null,
    readout.note ?? null,
  ];
  return parts.filter(Boolean).join(" · ");
}

export function PanelRings({
  memory,
  disk,
  cpu,
  onMemory,
  onDisk,
}: {
  memory: Readout;
  disk: Readout;
  cpu: Readout;
  onMemory: () => void;
  onDisk: () => void;
}) {
  const { t } = useI18n();
  return (
    <div
      className="mb-2 flex rounded-xl border border-hairline-soft"
      style={{
        backgroundImage: "linear-gradient(180deg, var(--color-chip-soft), transparent 62%)",
        backgroundColor: "var(--color-surface-raised)",
      }}
    >
      <RingCell
        label={t("nav.memory")}
        readout={memory}
        subLine={memory.status === "ok" ? `${memory.main} ${memory.mainTail ?? memory.unit}` : null}
        onOpen={onMemory}
      />
      <RingCell
        label={t("nav.disk")}
        readout={disk}
        subLine={disk.status === "ok" ? `${disk.main} ${disk.mainTail ?? disk.unit}` : null}
        onOpen={onDisk}
      />
      {/* CPU 在主窗口没有独立页签（统计页一段），所以不给跳转——不放假按钮 */}
      <RingCell label="CPU" readout={cpu} subLine={cpu.status === "ok" ? (cpu.sub[1] ?? cpu.sub[0] ?? null) : null} />
    </div>
  );
}
function RingCell({
  label,
  readout,
  subLine,
  onOpen,
}: {
  label: string;
  readout: Readout;
  /** 环下一行主读数；null = 读不到，走 note */
  subLine: string | null;
  onOpen?: () => void;
}) {
  const ok = readout.status === "ok";
  const frac = ok && readout.ratio != null ? Math.max(0, Math.min(1, readout.ratio)) : null;
  const color = toneColor[readout.tone];
  const pct = frac != null ? Math.round(frac * 100) : null;
  const note = subLine ?? readout.note ?? "—";

  const body: ReactNode = (
    <>
      <span className="relative grid size-[42px] place-items-center">
        <svg width={42} height={42} viewBox="0 0 42 42" className="absolute inset-0" aria-hidden="true">
          <circle cx="21" cy="21" r={R} fill="none" stroke="var(--color-chip)" strokeWidth={3.4} />
          {frac != null && frac > 0 ? (
            <circle
              cx="21"
              cy="21"
              r={R}
              fill="none"
              stroke={color}
              strokeWidth={3.4}
              strokeLinecap="round"
              strokeDasharray={`${frac * C} ${C - frac * C}`}
              transform="rotate(-90 21 21)"
            />
          ) : null}
        </svg>
        <span className="relative flex items-baseline gap-px">
          <b
            className="font-mono text-[13.5px] leading-none font-semibold tabular-nums"
            style={{ color: pct != null ? color : "var(--color-ink-faint)" }}
          >
            {pct ?? "—"}
          </b>
          {pct != null ? <small className="text-[8px] text-ink-faint">%</small> : null}
        </span>
      </span>
      <span className="mt-1 flex items-center gap-1 text-[10px] font-semibold text-ink-dim">
        {label}
        {readout.tag ? (
          <i
            className={`size-[5px] rounded-full ${
              readout.tag.tone === "danger"
                ? "bg-danger"
                : readout.tag.tone === "warn"
                  ? "bg-warn"
                  : "bg-accent"
            }`}
            title={readout.tag.text}
          />
        ) : null}
      </span>
      <span className="mt-0.5 max-w-full truncate font-mono text-[10px] leading-none tabular-nums text-ink-faint">
        {note}
      </span>
    </>
  );

  const cls =
    "flex min-w-0 flex-1 flex-col items-center px-1.5 pt-2.5 pb-2 text-center transition-colors [&+&]:border-l [&+&]:border-hairline-soft";

  if (!onOpen) return <div className={`${cls} cursor-default`} title={fullText(readout)}>{body}</div>;
  return (
    <button type="button" onClick={onOpen} title={fullText(readout)} className={`${cls} hover:bg-chip`}>
      {body}
    </button>
  );
}

/**
 * 端口三段带：监听 / 关注 / 暴露。暴露为 0 时该段不渲染（0 不占位）；
 * 未扫描整块退化成一行「未扫描」提示——不画三段 0。
 */
export function PanelPortBand({
  ports,
  onOpen,
}: {
  ports: PortSnapshot | null;
  onOpen: () => void;
}) {
  const { t } = useI18n();
  const counts = portCounts(ports);
  const conflicts = ports ? portConflicts(ports.entries).length : 0;

  if (!counts) {
    return (
      <button
        type="button"
        onClick={onOpen}
        className="mt-1 flex h-9 w-full items-center gap-2 rounded-[10px] border border-hairline-soft bg-chip-soft px-2.5 text-left transition-colors hover:bg-chip"
      >
        <span className="text-[10px] font-semibold text-ink-dim">{t("nav.ports")}</span>
        <span className="min-w-0 flex-1 truncate text-[9.5px] text-ink-faint">
          {t("common.state.notScanned")}
        </span>
        <span className="shrink-0 text-[9px] text-ink-faint/80">{t("panel.portsBand.scanHint")}</span>
      </button>
    );
  }

  return (
    <button
      type="button"
      onClick={onOpen}
      title={t("panel.portsBand.title", {
        listening: counts.listening,
        watched: counts.watched,
        exposed: counts.exposed,
        conns: counts.conns,
        conflicts,
      })}
      className="mt-1 flex h-10 w-full items-stretch rounded-[10px] border transition-colors"
      style={{
        borderColor: "color-mix(in srgb, var(--color-domain-port) 22%, transparent)",
        backgroundImage:
          "linear-gradient(100deg, color-mix(in srgb, var(--color-domain-port) 7%, transparent), transparent 70%)",
        backgroundColor: "var(--color-surface-raised)",
      }}
    >
      <BandSeg label={t("panel.portsBand.listening")} value={counts.listening} />
      <BandSeg label={t("panel.portsBand.watched")} value={counts.watched} />
      {counts.exposed > 0 ? (
        <BandSeg label={t("panel.portsBand.exposed")} value={counts.exposed} accent />
      ) : null}
    </button>
  );
}

function BandSeg({ label, value, accent }: { label: string; value: number; accent?: boolean }) {
  return (
    <span className="flex min-w-0 flex-1 items-center justify-center gap-1.5 border-l border-hairline-soft first:border-l-0">
      <span className="text-[9.5px] text-ink-faint">{label}</span>
      <span
        className={`font-mono text-[14px] leading-none font-semibold tabular-nums ${
          accent ? "text-[var(--color-domain-port)]" : "text-ink"
        }`}
      >
        {value}
      </span>
    </span>
  );
}
