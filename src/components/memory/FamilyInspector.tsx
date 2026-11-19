import { useMemo } from "react";

import { AppIcon } from "../AppIcon";
import { StackBar } from "../charts/StackBar";
import { Badge } from "../primitives";
import { useI18n } from "../../i18n/store";
import { formatBytes, formatPercent } from "../../lib/format";
import { useAppIcons } from "../../lib/useAppIcons";
import { swappedRatio, totalOf, type FamilyMem, type ProcMem } from "../../types/mem";

/**
 * 家族（应用层级）聚合详情（右栏，选中左栏家族行时呈现）：
 * 构成堆叠条（RSS + 换出 vs footprint，口径与 ProcessInspector 一致）+
 * 成员进程分摊列表（占家族比细条）。点成员下钻 ProcessInspector，右键走结束菜单。
 * 深探未全覆盖时如实标注「下界估算」——不把估算读数当精确值渲染。
 */
export function FamilyInspector({
  family,
  processes,
  onBack,
  onSelectPid,
  onKillMenu,
}: {
  family: FamilyMem;
  /** 全量进程快照：内部按 family.name 过滤出成员 */
  processes: ProcMem[];
  onBack: () => void;
  onSelectPid: (pid: number) => void;
  onKillMenu: (proc: ProcMem, x: number, y: number) => void;
}) {
  const i18n = useI18n();
  const { t } = i18n;

  const children = useMemo(
    () =>
      processes
        .filter((p) => p.family === family.name)
        .sort((a, b) => totalOf(b) - totalOf(a)),
    [processes, family.name],
  );

  // 复用左栏同一套图标缓存（模块级缓存，只增不改），不逐行发 IPC
  const icons = useAppIcons(
    useMemo(
      () => [
        {
          family: family.name,
          exe: processes.find((p) => p.family === family.name && p.exe)?.exe ?? null,
        },
      ],
      [family.name, processes],
    ),
  );

  const exact = family.footprint != null && family.deepCovered === family.processCount;
  const swapped = family.swapped ?? 0;
  const ratio = family.footprint != null ? swappedRatio(family) : null;
  const grand = Math.max(
    1,
    ...children.map((p) => totalOf(p)),
    family.footprint ?? 0,
    family.rss,
  );

  return (
    <div className="space-y-4">
      <div>
        <button
          type="button"
          onClick={onBack}
          className="text-[11px] text-accent transition-colors hover:text-ink"
        >
          {t("nav.backToOverview")}
        </button>
      </div>

      {/* 家族头：真实图标 + 家族名 + 进程数徽标 + 读数口径徽标 */}
      <div className="flex items-start gap-3">
        <AppIcon family={family.name} icon={icons[family.name]} size={28} className="shrink-0" />
        <div className="min-w-0 flex-1">
          <h1 className="truncate text-[15px] font-semibold tracking-tight">{family.name}</h1>
          <p className="mt-0.5 truncate text-[11px] text-ink-faint">
            {t("memory.families.procBadgeTip", { n: family.processCount })}
          </p>
        </div>
        {family.footprint == null ? (
          <Badge tone="warn" title={t("memory.badgeRssTip")}>
            {t("memory.badgeRss")}
          </Badge>
        ) : (
          <Badge tone="accent" title={t("memory.badgeFootprintTip")}>
            {t("memory.badgeFootprint")}
          </Badge>
        )}
      </div>

      {/* 家族聚合构成条：口径与 ProcessInspector 相同（footprint = RSS + 换出） */}
      {family.footprint != null && family.footprint > 0 ? (
        <section>
          <div className="mb-2 flex items-baseline justify-between">
            <span className="text-[11px] font-medium text-ink-dim">
              {t("memory.inspector.compositionTitle")}
            </span>
            <span className="text-[10px] text-ink-faint">
              {ratio != null ? t("memory.inspector.onDisk", { pct: formatPercent(ratio) }) : ""}
            </span>
          </div>
          <StackBar
            height={6}
            ariaLabel={t("memory.inspector.compositionAria", {
              rss: formatBytes(family.rss),
              swapped: formatBytes(swapped),
              footprint: formatBytes(family.footprint),
            })}
            segments={[
              {
                key: "rss",
                label: t("memory.inspector.rssSeg"),
                value: family.rss,
                tone: "neutral",
              },
              {
                key: "swapped",
                label: t("memory.inspector.swappedSeg"),
                value: swapped,
                tone: "warn",
              },
            ]}
            total={family.footprint}
          />
        </section>
      ) : null}

      {/* 成员分摊列表：占家族比细条 + 字节右对齐；点击下钻，右键结束 */}
      <section>
        <div className="mb-2 flex items-baseline justify-between">
          <span className="text-[11px] font-medium text-ink-dim">
            {t("memory.familyInspector.memberTitle", { n: children.length })}
          </span>
          {exact ? null : (
            <span
              className="text-[10px] text-ink-faint"
              title={t("memory.familyInspector.partialDeepTip")}
            >
              {t("memory.familyInspector.partialDeep", {
                covered: family.deepCovered,
                total: family.processCount,
              })}
            </span>
          )}
        </div>
        <ul className="overflow-hidden rounded-[10px] border border-hairline-soft">
          {children.map((p) => {
            const size = totalOf(p);
            const share = size / grand;
            return (
              <li key={`${p.pid}-${p.startTime}`}>
                <button
                  type="button"
                  onClick={() => onSelectPid(p.pid)}
                  onContextMenu={(e) => {
                    e.preventDefault();
                    onKillMenu(p, e.clientX, e.clientY);
                  }}
                  title={t("memory.familyInspector.memberHint")}
                  className="group flex w-full items-center gap-2.5 border-b border-hairline-soft px-3 py-2 text-left transition-colors last:border-b-0 hover:bg-chip"
                >
                  <span className="min-w-0 flex-1">
                    <span className="flex min-w-0 items-center gap-1.5">
                      <span className="min-w-0 truncate text-[12px] text-ink">{p.name}</span>
                      <span className="shrink-0 font-mono text-[9px] leading-none text-ink-faint">
                        {p.pid}
                      </span>
                      {p.workspace ? (
                        <span className="min-w-0 truncate text-[10px] text-ink-faint">
                          {p.workspace.label}
                        </span>
                      ) : null}
                    </span>
                    <span className="mt-[5px] block h-[2.5px] w-full max-w-[180px] overflow-hidden rounded-[2px] bg-chip">
                      <span
                        className="block h-full rounded-[2px] bg-info"
                        style={{ width: `${Math.min(100, share * 100)}%` }}
                      />
                    </span>
                  </span>
                  <span className="shrink-0 text-right">
                    <span className="block font-mono text-[11.5px] leading-none tabular-nums text-ink">
                      {formatBytes(size)}
                    </span>
                    <span className="mt-[5px] block text-[9px] leading-none text-ink-faint">
                      {t("memory.familyInspector.shareOf", { pct: formatPercent(share) })}
                    </span>
                  </span>
                </button>
              </li>
            );
          })}
        </ul>
      </section>
    </div>
  );
}
