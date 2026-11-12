/**
 * 详情页的资源小卡外壳（D4）：标题（域色图标 + 名称）+ 大数字 + 图表槽 + 一行脚注。
 *
 * 四张卡（CPU / 内存 / 磁盘 / 网速）共用它，保证高度、内边距、脚注位置一致；
 * 内部内容由调用方决定，外壳不读任何数据。
 */
import type { ReactNode } from "react";

export function ResourceTile({
  label,
  dotColor,
  icon,
  value,
  chart,
  note,
  hint,
}: {
  label: string;
  /** 域色：贯穿图标与图表 */
  dotColor: string;
  icon?: ReactNode;
  value: ReactNode;
  chart?: ReactNode;
  /** 脚注：口径一句话（必须能回答「这个数字怎么来的」） */
  note: string;
  hint?: string;
}) {
  return (
    <section
      className="rounded-xl border border-hairline-soft bg-chip-soft px-3 py-2.5"
      title={hint}
    >
      <div className="flex items-center gap-1.5 text-[9.5px] text-ink-faint">
        <span
          aria-hidden
          className="inline-block size-[6px] rounded-full"
          style={{ background: dotColor }}
        />
        {icon}
        <span>{label}</span>
      </div>
      <div className="mt-1 font-mono text-[19px] leading-none tracking-tight text-ink tabular-nums">
        {value}
      </div>
      <div className="mt-2 min-h-[26px]">{chart}</div>
      <div className="mt-1.5 text-[9px] leading-snug text-ink-faint">{note}</div>
    </section>
  );
}
