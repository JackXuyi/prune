import { splitSpeed, type NetReadout } from "../../lib/statsReadout";
import { Sparkline } from "../Sparkline";

/**
 * 网络卡主体（M16，主窗口 960 用）：双读数 + 各自归一化的走势。
 *
 * 上下行量级常差 10 倍，共享纵轴会把上传压成一条直线，所以两条线各自画、
 * 各自归一化；再加上 `baselineZero`，空闲时的几十 KB/s 抖动不会被放大成满屏尖峰。
 */
export function NetReading({ readout }: { readout: NetReadout }) {
  const ok = readout.status === "ok";
  const extra = readout.sub.slice(1); // sub[0] 是「近 5 min 平均」，与下面的 sparkline 重复

  return (
    <div>
      <div className="grid grid-cols-2 gap-6">
        <Direction
          arrow="↓"
          tone="accent"
          bps={readout.downBps}
          series={readout.downSeries}
          ok={ok}
          label="下载（物理网卡合计）"
        />
        <Direction
          arrow="↑"
          tone="info"
          bps={readout.upBps}
          series={readout.upSeries}
          ok={ok}
          label="上传（物理网卡合计）"
        />
      </div>
      <div className="mt-1 flex flex-wrap items-center gap-x-3 gap-y-0.5 text-[10px] text-ink-faint">
        {ok ? (
          <>
            <span>
              近 5 min{" "}
              <span className="text-ink-dim">
                {readout.avgDownText} / {readout.avgUpText}
              </span>
            </span>
            {extra.map((s) => (
              <span key={s}>{s}</span>
            ))}
          </>
        ) : (
          <span>{readout.note ?? "—"}</span>
        )}
      </div>
    </div>
  );
}

function Direction({
  arrow,
  tone,
  bps,
  series,
  ok,
  label,
}: {
  arrow: string;
  tone: "accent" | "info";
  bps: number;
  series: number[];
  ok: boolean;
  label: string;
}) {
  const { value, unit } = splitSpeed(bps);
  const text = tone === "accent" ? "text-accent" : "text-info";

  return (
    <div title={label}>
      <div className="flex items-baseline gap-1.5">
        <span className={`text-[12px] leading-none ${text}`}>{arrow}</span>
        {ok ? (
          <>
            <span className={`font-mono text-[19px] leading-none tabular-nums ${text}`}>{value}</span>
            <span className="text-[10px] text-ink-faint">{unit}</span>
          </>
        ) : (
          <span className="font-mono text-[19px] leading-none text-ink-faint">—</span>
        )}
      </div>
      <div className="mt-1 h-[24px]">
        {series.length >= 2 ? (
          <Sparkline values={series} height={24} tone={tone} baselineZero />
        ) : (
          <div className="flex h-[24px] items-center text-[9.5px] text-ink-faint/70">
            {ok ? "走势积累中（每秒一个点）" : "—"}
          </div>
        )}
      </div>
    </div>
  );
}
