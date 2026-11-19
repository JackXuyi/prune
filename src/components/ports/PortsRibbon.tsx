import { useI18n } from "../../i18n/store";
import type { PortSnapshot } from "../../types/net";
import {
  ActivityIcon,
  GlobeIcon,
  LinkIcon,
  PortJackIcon,
  WarnTriangleIcon,
} from "../icons";
import { RibbonChip, RibbonSeg, RibbonShell, RibbonValue } from "../Ribbon";

/**
 * 端口页状态带（eul 重设计 · ports-eul-mockup.html ①）：
 * 5 段全是本页任务内读数——监听（端口域色琥珀）/ 对外暴露 / 关注 /
 * 活跃连接 / CLOSE_WAIT。归属盲区不上带子（口径信息，降级为覆盖提醒带）。
 */
export function PortsRibbon({ snapshot }: { snapshot: PortSnapshot }) {
  const { t } = useI18n();
  const entries = snapshot.entries;
  const listen = entries.length;
  const exposed = entries.filter((e) => e.exposure === "any").length;
  const watched = entries.filter((e) => e.watched).length;
  const udp = entries.filter((e) => e.proto === "udp").length;
  let conns = 0;
  for (const e of entries) if (e.conns != null) conns += e.conns;
  let closeWait = 0;
  for (const c of Object.values(snapshot.connsByPid)) closeWait += c.closeWait;

  return (
    <RibbonShell ariaLabel={t("ports.ribbon.aria")}>
      <RibbonSeg
        title={t("ports.ribbon.listenTitle", {
          n: listen,
          attributed: snapshot.coverage.attributed,
          total: snapshot.coverage.listenTotal,
        })}
      >
        <RibbonChip color="var(--color-domain-port)" icon={<PortJackIcon size={13} />} />
        <RibbonValue value={String(listen)} unit={t("ports.ribbon.listenUnit")} />
      </RibbonSeg>
      <RibbonSeg title={t("ports.ribbon.exposedTitle", { n: exposed })}>
        <RibbonChip color="var(--color-warn)" icon={<GlobeIcon size={13} />} />
        <RibbonValue value={String(exposed)} unit={t("ports.ribbon.exposedUnit")} tone="warn" />
      </RibbonSeg>
      <RibbonSeg title={t("ports.ribbon.watchedTitle", { n: watched })}>
        <RibbonChip color="var(--color-accent)" icon={<LinkIcon size={13} />} />
        <RibbonValue value={String(watched)} unit={t("ports.ribbon.watchedUnit")} />
      </RibbonSeg>
      <RibbonSeg
        title={t("ports.ribbon.connsTitle", { n: conns, udp: t("ports.ribbon.udpNote", { n: udp }) })}
      >
        <RibbonChip color="var(--color-domain-net)" icon={<ActivityIcon size={13} />} />
        <RibbonValue value={String(conns)} unit={t("ports.ribbon.connsUnit")} />
      </RibbonSeg>
      <RibbonSeg title={t("ports.ribbon.closeWaitTitle", { n: closeWait })}>
        <RibbonChip color="var(--color-danger)" icon={<WarnTriangleIcon size={13} />} />
        <RibbonValue value={String(closeWait)} unit={t("ports.ribbon.closeWaitUnit")} tone="danger" />
      </RibbonSeg>
    </RibbonShell>
  );
}
