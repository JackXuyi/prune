/**
 * 空态插画（UI v4 §6.7 / v5 §3.7）：细线稿 + 呼吸微光。
 *
 * 「很干净」比一段灰字更有情绪；插画是装饰不是信息，全部 aria-hidden。
 * 尺寸由调用方控制，线稿用 currentColor 适配主题。
 *
 * v5 新增 4 个语义变体 —— 空态必须能区分「没扫到」和「没有数据」，
 * 插画是这套语义的第一眼信号，不能四个场景共用一把剪刀。
 */
export type EmptyArtKind = "scan" | "search" | "no-record" | "no-device";

/** 剪刀线稿：默认空态（「已经很干净」） */
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

/**
 * 四个语义空态插画，统一 64×64 viewBox + 1.5px 线宽。
 * 变化点在「主体图形」，不在装饰密度 —— 四张图放在一起要像一套。
 */
export function EmptyArt({
  kind,
  size = 64,
  className = "",
}: {
  kind: EmptyArtKind;
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
      <g stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
        {kind === "scan" ? (
          /* 尚未扫描：雷达弧 + 扫描线 + 中心点 */
          <>
            <circle cx="32" cy="32" r="6" opacity="0.9" />
            <path d="M32 32 L49 21" opacity="0.85" />
            <path d="M20 20 A17 17 0 0 1 44 20" opacity="0.5" />
            <path d="M14 14 A25 25 0 0 1 50 14" opacity="0.28" />
            <path d="M32 20 a12 12 0 0 1 12 12" strokeDasharray="2.5 3" opacity="0.55" />
            <circle cx="32" cy="32" r="1.8" fill="currentColor" stroke="none" />
          </>
        ) : kind === "search" ? (
          /* 搜索无果：放大镜 + 内部空白斜线 */
          <>
            <circle cx="28" cy="28" r="13" />
            <path d="M37.5 37.5 L50 50" strokeWidth="1.8" />
            <path d="M21.5 24.5 L34.5 31.5" strokeWidth="1" opacity="0.4" strokeDasharray="2 3" />
          </>
        ) : kind === "no-record" ? (
          /* 没有记录：带折角的空清单 */
          <>
            <path d="M18 12 h20 l8 8 v32 a2 2 0 0 1 -2 2 H18 a2 2 0 0 1 -2 -2 V14 a2 2 0 0 1 2 -2 Z" />
            <path d="M38 12 v8 h8" opacity="0.7" />
            <path d="M23 30 h18" strokeWidth="1" opacity="0.35" />
            <path d="M23 37 h18" strokeWidth="1" opacity="0.35" />
            <path d="M23 44 h10" strokeWidth="1" opacity="0.35" />
          </>
        ) : (
          /* 没有设备：显示器 + 断开的信号弧 */
          <>
            <rect x="12" y="18" width="30" height="21" rx="2" />
            <path d="M22 45 h10" opacity="0.7" />
            <path d="M27 39 v6" opacity="0.7" />
            <path d="M48 24 a7 7 0 0 1 0 10" opacity="0.5" />
            <path d="M52 19 a13 13 0 0 1 0 20" opacity="0.28" />
            <path d="M45 26 l7 7" strokeDasharray="2 3" opacity="0.25" />
          </>
        )}
      </g>
    </svg>
  );
}

/**
 * 空态组合：插画 + 主句 + 补注，居中排布，供各屏空态复用。
 *
 * 兼容性：老调用方只传 title/note —— 不传 art 时仍用剪刀线稿，
 * 行为与 v4 完全一致；v5 页面逐页补 art 即可平滑迁移。
 */
export function EmptyState({
  art,
  title,
  note,
  description,
  footnote,
  action,
}: {
  art?: EmptyArtKind;
  title: string;
  /** v4 的名字，保留 */
  note?: string;
  /** v5 的名字，优先于 note */
  description?: string;
  footnote?: string;
  action?: React.ReactNode;
}) {
  const body = description ?? note;
  return (
    <div className="flex flex-col items-center justify-center gap-2.5 py-10 text-center">
      {art ? (
        <EmptyArt kind={art} className="prune-anim text-ink-faint/70" />
      ) : (
        <ScissorsArt className="prune-anim text-accent/70" />
      )}
      <p className="text-[12.5px] font-medium text-ink-dim">{title}</p>
      {body ? (
        <p className="max-w-[340px] text-[10.5px] leading-relaxed text-ink-faint">{body}</p>
      ) : null}
      {action}
      {footnote ? <p className="mt-1 text-[9.5px] text-ink-faint">{footnote}</p> : null}
    </div>
  );
}
