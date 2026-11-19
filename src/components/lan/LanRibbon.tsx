import { useI18n } from "../../i18n/store";
import type { LanSnapshot } from "../../types/lan";
import { GlobeIcon, WifiIcon } from "../icons";
import { RibbonChip, RibbonSeg, RibbonShell, RibbonValue } from "../Ribbon";

/**
 * 局域网状态带（eul 重设计 · ports-eul-mockup.html 局域网子页 ①）：
 * 4 段——在线（net 青）/ 对我开放的端口 / 新面孔 / 探测覆盖口径。
 *
 * 「探测 12/23」是口径不是进度：邻居表 23 台里本轮只主动探测了 12 个地址；
 * 没探测时显示「仅邻居表」而不是画成 0（诚实红线）。
 */
export function LanRibbon({ snapshot }: { snapshot: LanSnapshot }) {
  const { t } = useI18n();
  const online = snapshot.devices.filter((d) => d.online).length;
  const newFaces = snapshot.devices.filter((d) => d.isNew).length;
  const exposed = snapshot.exposedPorts.length;

  return (
    <RibbonShell ariaLabel={t("lan.ribbon.aria")}>
      <RibbonSeg
        title={t("lan.ribbon.onlineTitle", { n: online, total: snapshot.devices.length })}
      >
        <RibbonChip color="var(--color-domain-net)" icon={<WifiIcon size={13} />} />
        <RibbonValue
          value={String(online)}
          unit={t("lan.ribbon.onlineUnit", { n: snapshot.devices.length })}
        />
      </RibbonSeg>
      <RibbonSeg title={t("lan.ribbon.exposedTitle", { n: exposed })}>
        <RibbonChip color="var(--color-warn)" icon={<GlobeIcon size={13} />} />
        <RibbonValue value={String(exposed)} unit={t("lan.ribbon.exposedUnit")} tone={exposed > 0 ? "warn" : undefined} />
      </RibbonSeg>
      <RibbonSeg title={t("lan.ribbon.newFacesTitle", { n: newFaces })}>
        <RibbonChip
          color="var(--color-accent)"
          icon={
            <svg width={13} height={13} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2} strokeLinecap="round" aria-hidden>
              <circle cx="12" cy="12" r="8.5" />
              <path d="M12 8.5v7M8.5 12h7" />
            </svg>
          }
        />
        <RibbonValue value={String(newFaces)} unit={t("lan.ribbon.newFacesUnit")} />
      </RibbonSeg>
      <RibbonSeg
        title={t("lan.ribbon.coverageTitle", {
          scanned: snapshot.scanned,
          total: snapshot.total,
        })}
      >
        <RibbonChip
          color="var(--color-ink-faint)"
          icon={
            <svg width={13} height={13} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2} strokeLinecap="round" aria-hidden>
              <circle cx="11" cy="11" r="7" />
              <path d="m20 20-3.5-3.5" />
            </svg>
          }
        />
        {snapshot.scanned > 0 ? (
          <RibbonValue
            value={String(snapshot.scanned)}
            unit={t("lan.ribbon.coverageUnit", { total: snapshot.total })}
          />
        ) : (
          <RibbonValue value="—" unit={t("lan.ribbon.neighborsOnly")} />
        )}
      </RibbonSeg>
    </RibbonShell>
  );
}
