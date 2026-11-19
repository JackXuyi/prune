import { DiagTrackCard } from "./DiagTrackCard";
import { SitesSection } from "./sites/SitesSection";
import { SpeedHero } from "./SpeedHero";
import { TestRibbon } from "./TestRibbon";

/**
 * 网络测试二级页（2026-11-19 重设计，设计稿 output/design/net-test-eul-mockup.html）：
 * 链路诊断 + 带宽测速 + 站点检测三件事合并——它们都是**手动发起的主动测试**，
 * 回答的是同一个三层问题：链路通吗（诊断）→ 带宽多少（测速）→ 站点快吗（检测）。
 *
 * 结构自上而下：速读条（62）→ 测速 Hero（128）→ 诊断轨道卡（84）→ 站点区。
 * 卡片多时**页面整体滚动**（NetBoard 外层统一滚动，站点区不内部滚）。监测页维持现状，见 MonitorPage。
 */
export function TestPage() {
  return (
    <div className="flex flex-col gap-2.5 pt-3">
      <TestRibbon />
      <SpeedHero />
      <DiagTrackCard />
      <SitesSection />
    </div>
  );
}
