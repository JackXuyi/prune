import type { ReactNode } from "react";

/**
 * 网络页小件（监测 / 测试两个二级页共用）：
 * SectionHead（区块标题行）、GhostButton（描边小按钮）、rpmTone、GIGABIT_BPS。
 * 从 NetBoard 抽出来：测试页与监测页都要用，不再各写一份。
 */

/** 区块标题行：11px 标题 + 右侧一行口径 */
export function SectionHead({ title, hint }: { title: string; hint: string }) {
  return (
    <div className="flex items-baseline justify-between">
      <span className="text-[11px] font-medium tracking-wide text-ink-dim">{title}</span>
      <span className="text-[10px] text-ink-faint">{hint}</span>
    </div>
  );
}

/**
 * 描边小按钮（10.5px）：网络页内出现 ≥6 次的同一形态（开始诊断 / 开始测速 /
 * 全部测试 / 编辑 / 取消…），提为一个组件。`variant` 只给语义色，不发明新样式。
 */
export function GhostButton({
  onClick,
  disabled,
  title,
  variant = "default",
  children,
}: {
  onClick: () => void;
  disabled?: boolean;
  title?: string;
  /** default 中性 · go 主操作（info）· stop 停止（warn）· primary 域色描边 */
  variant?: "default" | "go" | "stop" | "primary";
  children: ReactNode;
}) {
  const tone =
    variant === "go"
      ? "text-ink border-info/40"
      : variant === "stop"
        ? "text-warn border-warn/40"
        : variant === "primary"
          ? "text-ink border-domain-net/45"
          : "text-ink-dim border-hairline-soft";
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      title={title}
      className={`shrink-0 rounded-md border px-2.5 py-1 text-[10.5px] transition-colors hover:bg-chip disabled:cursor-default disabled:opacity-50 ${tone}`}
    >
      {children}
    </button>
  );
}

/** Apple RPM 分级 → 徽标语义色（只做文案色映射，不发明新阈值） */
export function rpmTone(level: string): "danger" | "warn" | "accent" | "neutral" {
  switch (level) {
    case "Poor":
      return "danger";
    case "Low":
      return "warn";
    case "Medium":
    case "Good":
      return "accent";
    default:
      return "neutral";
  }
}

/** 测速条的归一化参考：1 Gbps 扣 TCP/IP 头开销的实际可用带宽（~976 Mbps） */
export const GIGABIT_BPS = (1e9 * (1 - 0.024)) / 8;

/** Mbps 拆「数值 + 单位」：主读数数字单独占位，单位小一号 */
export function splitMbps(bps: number): { value: string; unit: string } {
  const mbps = bps / 1_000_000;
  return { value: mbps >= 100 ? String(Math.round(mbps)) : mbps.toFixed(1), unit: "Mbps" };
}
