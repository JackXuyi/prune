/**
 * 分段轨道（UI v4 §5.5）：网络诊断 DNS → 网关 → 出口三层探测的图形化。
 *
 * 节点逐段点亮（ok=绿勾 / pending=灰点 / fail=红叉），连接线只对
 * 「两端都 ok」的区间渲染渐变实线——没探完就不画「通过」，不造假事实。
 */
export interface TrackNode {
  label: string;
  /** 节点下的读数（如「DNS 12ms」） */
  sub?: string;
  state: "ok" | "pending" | "fail";
}

const OK = "var(--color-accent)";
const FAIL = "var(--color-danger)";
const PENDING = "var(--color-ink-faint)";

export function TrackProgress({
  nodes,
  className = "",
  ariaLabel,
}: {
  nodes: TrackNode[];
  className?: string;
  ariaLabel?: string;
}) {
  return (
    <div className={`flex items-start ${className}`} role="img" aria-label={ariaLabel}>
      {nodes.map((n, i) => {
        const color = n.state === "ok" ? OK : n.state === "fail" ? FAIL : PENDING;
        const prev = i > 0 ? nodes[i - 1] : null;
        const lineOk = prev != null && prev.state === "ok" && n.state === "ok";
        return (
          <div key={i} className="flex items-start" style={{ flex: i === nodes.length - 1 ? "0 0 auto" : "1 0 auto" }}>
            {i > 0 ? (
              <div
                className="mt-[13px] h-[2px] min-w-[36px] flex-1 rounded-full"
                style={{
                  background: lineOk
                    ? `linear-gradient(90deg, ${OK}, color-mix(in srgb, ${OK} 40%, transparent))`
                    : n.state === "fail" && prev?.state === "ok"
                      ? `linear-gradient(90deg, ${OK}, ${FAIL})`
                      : "var(--color-chip)",
                  marginInline: 6,
                }}
              />
            ) : null}
            <div className="flex min-w-[52px] flex-col items-center gap-1.5">
              <span
                className="grid size-[26px] place-items-center rounded-full border text-[11px] font-semibold"
                style={{
                  color,
                  borderColor: `color-mix(in srgb, ${color} 40%, transparent)`,
                  background: `color-mix(in srgb, ${color} 12%, transparent)`,
                  boxShadow: n.state === "ok" ? `0 0 10px color-mix(in srgb, ${OK} 25%, transparent)` : undefined,
                }}
              >
                {n.state === "ok" ? "✓" : n.state === "fail" ? "✕" : "·"}
              </span>
              <span className="font-mono text-[9.5px] leading-tight text-ink-dim">{n.sub ?? n.label}</span>
            </div>
          </div>
        );
      })}
    </div>
  );
}
