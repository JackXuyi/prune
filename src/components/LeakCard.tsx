import { formatBytes, formatPercent } from "../lib/format";
import { totalGrowth, type LeakItem, type LeakReport } from "../types/mem";
import { ChevronRightIcon } from "./icons";
import { Badge } from "./primitives";

/**
 * 泄漏嫌疑的证据展示（M3 / B5）。
 *
 * 文案硬约束（功能方案 §6.3）——改这个文件前先读：
 * - 只能说「疑似内存泄漏」，**永远不说**「你的 XX 泄漏了」；
 * - 必须展示依据（增长量 + 时长 + R²），只给结论等于制造焦虑；
 * - 不允许出现「释放 / 加速 / 优化」这类词。
 *
 * 误报一次的信任成本远高于漏报一次，所以这里是「证据优先」的排版：
 * 数字在前，结论在后，且措辞留有余地。
 */

function windowLabel(secs: number): string {
  if (secs >= 3600) return `${Math.round(secs / 3600)} 小时`;
  return `${Math.round(secs / 60)} min`;
}

/** 一句话证据：近 30 min 增长 480 MB（+51%）· 斜率 960 MB/h · R² 0.86 */
export function evidenceLine(item: LeakItem): string {
  return `近 ${windowLabel(item.windowSecs)}增长 ${formatBytes(item.growth)}（+${formatPercent(
    item.growthRatio,
  )}）· 斜率 ${formatBytes(item.slopeMbPerHour * 1e6)}/h · R² ${item.r2.toFixed(2)}`;
}

/** 面板用：一行提示条，只在有命中时出现 */
export function LeakBanner({
  report,
  onOpen,
}: {
  report: LeakReport;
  onOpen: () => void;
}) {
  const items = report.items;
  if (items.length === 0) return null;

  const families = new Set(items.map((i) => i.family));
  const window = windowLabel(items[0].windowSecs);

  return (
    <button
      type="button"
      onClick={onOpen}
      title={items.map((i) => `${i.name}：${evidenceLine(i)}`).join("\n")}
      className="mb-2 flex w-full items-center gap-2 rounded-lg border border-warn/25 bg-warn/[0.07] px-2.5 py-2 text-left transition-colors hover:bg-warn/[0.11]"
    >
      <span className="min-w-0 flex-1">
        <span className="block text-[11px] leading-tight text-warn">疑似内存泄漏</span>
        <span className="mt-0.5 block text-[10px] leading-tight text-ink-faint">
          {families.size} 个应用 · {items.length} 个进程 · {window}合计 +
          {formatBytes(totalGrowth(items))}
        </span>
      </span>
      <ChevronRightIcon size={12} className="shrink-0 text-warn/70" />
    </button>
  );
}

/** 主窗口用：完整证据卡（每行一个进程，点进去看曲线） */
export function LeakCard({
  report,
  onOpen,
}: {
  report: LeakReport;
  onOpen: (pid: number) => void;
}) {
  // 无法判定时必须说出来，不能让用户以为「检查过、没问题」
  if (report.note) {
    return (
      <p className="rounded-lg border border-hairline-soft bg-surface-raised/40 px-3 py-2 text-[11px] leading-relaxed text-ink-faint">
        泄漏判定当前不可用：{report.note}
      </p>
    );
  }

  if (report.items.length === 0) return null;

  return (
    <section className="rounded-lg border border-warn/25 bg-warn/[0.05] p-3">
      <div className="flex items-baseline justify-between gap-2">
        <div className="flex items-center gap-1.5">
          <span className="text-[11px] font-medium text-warn">疑似内存泄漏</span>
          <Badge tone="warn" title="统计判断，不是结论；依据见每行的增长量、斜率与 R²">
            按曲线判断
          </Badge>
        </div>
        <span className="text-[10px] text-ink-faint">
          检查了 {report.observed} 个进程 · {report.items.length} 个命中
        </span>
      </div>

      <ul className="mt-1.5">
        {report.items.map((item) => (
          <li key={`${item.pid}-${item.startTime}`}>
            <button
              type="button"
              onClick={() => onOpen(item.pid)}
              className="flex w-full items-center gap-2 rounded-md px-1 py-1.5 text-left transition-colors hover:bg-white/[0.05]"
            >
              <span className="min-w-0 flex-1">
                <span className="block truncate text-[12px] leading-tight text-ink">
                  {item.name}
                  <span className="ml-1.5 text-[10px] text-ink-faint">{item.family}</span>
                  {item.gcRuntime ? (
                    <span className="ml-1.5 text-[10px] text-ink-faint" title="GC 语言：门槛翻倍至 300 MB / 60 min">
                      GC 语言
                    </span>
                  ) : null}
                </span>
                <span className="mt-0.5 block truncate text-[10px] leading-tight text-ink-faint">
                  {evidenceLine(item)}
                </span>
              </span>
              <span className="shrink-0 text-right">
                <span className="block font-mono text-[11px] tabular-nums text-warn">
                  +{formatBytes(item.growth)}
                </span>
                <span className="mt-0.5 block font-mono text-[10px] tabular-nums text-ink-faint">
                  现 {formatBytes(item.footprint)}
                </span>
              </span>
              <ChevronRightIcon size={11} className="shrink-0 text-ink-faint" />
            </button>
          </li>
        ))}
      </ul>

      <p className="mt-1.5 text-[10px] leading-relaxed text-ink-faint">
        判据：存在超 {windowLabel(report.windowSecs)}、净增长超门槛且占比 &gt; 40%、
        线性回归 R² &gt; 0.70、且不是一次性跃升。浏览器、虚拟机与系统进程不在判定范围内。
        这只是嫌疑，建议重启该应用后观察是否复现。
      </p>
    </section>
  );
}

/** 检视器用：选中进程命中时展示完整证据 */
export function LeakEvidence({ item }: { item: LeakItem }) {
  return (
    <section className="rounded-lg border border-warn/25 bg-warn/[0.05] p-3.5">
      <div className="flex items-baseline justify-between">
        <span className="text-[11px] font-medium text-warn">疑似内存泄漏</span>
        <span className="text-[10px] text-ink-faint">按曲线判断，不是结论</span>
      </div>
      <dl className="mt-2 space-y-1.5 text-[11px]">
        <EvRow label="窗口" value={`近 ${windowLabel(item.windowSecs)}（${item.points} 个采样点）`} />
        <EvRow
          label="净增长"
          value={`${formatBytes(item.growth)}（+${formatPercent(item.growthRatio)}）`}
        />
        <EvRow label="斜率" value={`${formatBytes(item.slopeMbPerHour * 1e6)}/h`} />
        <EvRow label="拟合优度" value={`R² ${item.r2.toFixed(3)}`} />
        <EvRow
          label="最大单点跃升"
          value={`占净增长 ${formatPercent(item.stepRatio)}（越低越像持续爬升）`}
        />
        {item.gcRuntime ? (
          <EvRow label="门槛" value="GC 语言：300 MB / 60 min（普通进程 150 MB / 30 min）" />
        ) : null}
      </dl>
      <p className="mt-2 text-[10px] leading-relaxed text-ink-faint">
        统计特征符合「持续增长」，但也可能是缓存累积。建议重启该应用后观察是否复现。
      </p>
    </section>
  );
}

function EvRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-start gap-3">
      <dt className="w-20 shrink-0 text-ink-faint">{label}</dt>
      <dd className="min-w-0 flex-1 text-ink-dim">{value}</dd>
    </div>
  );
}
