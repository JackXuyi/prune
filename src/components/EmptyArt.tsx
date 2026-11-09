/**
 * 空态插画（UI v4 §6.7）：剪刀线稿 + 呼吸微光。
 *
 * 「很干净」比一段灰字更有情绪；插画是装饰不是信息，全部 aria-hidden。
 * 尺寸由调用方控制，线稿用 currentColor 适配主题。
 */
export function ScissorsArt({
  size = 56,
  className = "",
}: {
  size?: number;
  className?: string;
}) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 64 64"
      fill="none"
      className={className}
      aria-hidden
    >
      {/* 剪刀：两个环柄 + 交叉刀刃，1.5px 细线 */}
      <g stroke="currentColor" strokeWidth="1.5" strokeLinecap="round">
        {/* 上环柄 */}
        <circle cx="17" cy="17" r="7" />
        {/* 下环柄 */}
        <circle cx="17" cy="47" r="7" />
        {/* 交叉刀刃：从环柄右侧交于轴心 */}
        <path d="M22.6 21.4 L46 34" />
        <path d="M22.6 42.6 L46 30" />
        {/* 轴心铆钉 */}
        <circle cx="38" cy="32" r="2.2" fill="currentColor" stroke="none" />
        {/* 刀尖处的小剪裁虚线：示意「正在剪掉冗余」 */}
        <path d="M48 33 l8 0" strokeDasharray="2.5 3" opacity="0.5" />
      </g>
    </svg>
  );
}

/** 空态组合：插画 + 主句 + 补注，居中排布，供各屏空态复用 */
export function EmptyState({
  title,
  note,
  action,
}: {
  title: string;
  note?: string;
  action?: React.ReactNode;
}) {
  return (
    <div className="flex flex-col items-center justify-center gap-2.5 py-10 text-center">
      <ScissorsArt className="prune-anim text-accent/70" />
      <p className="text-[12.5px] font-medium text-ink-dim">{title}</p>
      {note ? <p className="max-w-[340px] text-[10.5px] leading-relaxed text-ink-faint">{note}</p> : null}
      {action}
    </div>
  );
}
