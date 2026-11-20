import { useI18n } from "../../i18n/store";
import type { AudioDevice, BTDevice, USBDevice } from "../../types/peripheral";
import { Badge } from "../primitives";
import { DeviceIcon, HeadphonesIcon, UsbIcon } from "../icons";
import { MeterBar } from "../charts/MeterBar";
import { MiniGauge } from "../stats/MiniGauge";
import { usePeripherals } from "./usePeripherals";

/** 外设域色（与 rail「外设」导航项同源） */
const EP_COLOR = "var(--color-domain-device)";

/**
 * 外设页（m21，docs/m21-peripherals-plan.md）：
 * 接入本机的外部设备——蓝牙（AirPods 分件电量）/ USB / 音频。
 *
 * 结构：蓝牙卡（整宽）→ USB + 音频双列。
 * 纪律：
 * - 读不到电量的部件不画环（MiniGauge 空轨道只出现在"知道设备但不知道电量"时）；
 * - 无设备的分区整块隐藏——隐藏是失败语义，绝不渲染「0 台设备」；
 * - 采样纪律：usePeripherals 挂载开 / 卸载停，页面无全局刷新按钮（同设备页）。
 */
export function PeripheralsBoard() {
  const { t } = useI18n();
  const { snapshot, enabled } = usePeripherals();

  const bt = snapshot?.bt ?? [];
  const usb = snapshot?.usb ?? [];
  const audio = snapshot?.audio ?? [];

  // 首轮采样未到 / 开关关闭：给一行状态说明，不画假设备
  if (!snapshot) {
    return (
      <div className="min-h-0 flex-1 overflow-x-hidden overflow-y-auto px-5 pt-3 pb-3">
        <p className="mt-10 text-center text-[11px] text-ink-faint">
          {enabled ? t("peripherals.waiting") : t("peripherals.switchOff")}
        </p>
      </div>
    );
  }

  const hasBt = snapshot.btAt != null && bt.length > 0;
  const hasUsb = snapshot.usbAt != null && usb.length > 0;
  const hasAudio = snapshot.audioAt != null && audio.length > 0;

  return (
    <div className="min-h-0 flex-1 overflow-x-hidden overflow-y-auto px-5 pt-3 pb-3">
      <div className="flex flex-col gap-[9px]">
        {hasBt ? <BtCard devices={bt} /> : null}
        {hasUsb || hasAudio ? (
          <div className="grid shrink-0 grid-cols-2 gap-2.5">
            {hasUsb ? <UsbCard devices={usb} /> : null}
            {hasAudio ? <AudioCard devices={audio} /> : null}
          </div>
        ) : null}
      </div>
    </div>
  );
}

// ---------------------------------------------------------------- 卡片壳

/**
 * 分区卡壳（对齐 InfoTile 的卡面形态，但内容是变长列表）：
 * 图标芯片 + 标题 + 变长行；完整口径走 tooltip（三层文字以内）。
 */
function SectionCard({
  title,
  tip,
  icon,
  children,
}: {
  title: string;
  tip: string;
  icon: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <div
      className="group relative flex h-full w-full flex-col overflow-hidden rounded-card border border-hairline-soft bg-surface-raised px-3.5 py-2.5"
      title={tip}
    >
      <div className="flex w-full items-center">
        <span
          className="grid size-[24px] shrink-0 place-items-center rounded-[7px]"
          style={{
            background: `color-mix(in srgb, ${EP_COLOR} 15%, transparent)`,
            color: EP_COLOR,
            boxShadow: `0 0 10px color-mix(in srgb, ${EP_COLOR} 20%, transparent)`,
          }}
          aria-hidden
        >
          {icon}
        </span>
        <span className="ml-2 min-w-0 flex-1 truncate text-[11px] font-semibold text-ink-dim">
          {title}
        </span>
      </div>
      <div className="mt-2 flex min-w-0 flex-1 flex-col gap-[7px]">{children}</div>
    </div>
  );
}

// ---------------------------------------------------------------- 蓝牙

function BtCard({ devices }: { devices: BTDevice[] }) {
  const { t } = useI18n();
  return (
    <SectionCard
      title={t("peripherals.btTitle")}
      tip={t("peripherals.btTip")}
      icon={<HeadphonesIcon size={13} />}
    >
      {devices.map((d) => (
        <BTDeviceRow key={`${d.name}:${d.connected}`} device={d} />
      ))}
    </SectionCard>
  );
}

/**
 * 单设备行（三分支）：
 * - Apple 音频（有分件电量）→ 名称行 + 左/右/盒三个 MiniGauge 环；
 * - 有主电量（键盘 / 鼠标 / 第三方耳机）→ MeterBar 电量条；
 * - 无电量 → 名称 + 连接态。
 */
function BTDeviceRow({ device }: { device: BTDevice }) {
  const { t } = useI18n();
  const d = device;

  const nameRow = (
    <div className="flex min-w-0 items-center gap-2">
      {d.deviceType.toLowerCase().includes("audio") || d.batteryLeft != null ? (
        <HeadphonesIcon size={12} className="shrink-0 text-ink-faint" />
      ) : (
        <DeviceIcon size={12} className="shrink-0 text-ink-faint" />
      )}
      <span className="min-w-0 flex-1 truncate text-[10.5px] text-ink-dim" title={d.name}>
        {d.name}
      </span>
      {!d.connected ? (
        <span className="shrink-0 text-[9px] text-ink-faint">{t("peripherals.notConnected")}</span>
      ) : null}
    </div>
  );

  // AirPods 类：任一分件有值即画环组（读不到的部件不画环）
  const parts = [
    { key: "left", label: t("peripherals.partLeft"), pct: d.batteryLeft },
    { key: "right", label: t("peripherals.partRight"), pct: d.batteryRight },
    { key: "case", label: t("peripherals.partCase"), pct: d.batteryCase },
  ].filter((p) => p.pct != null);

  if (parts.length > 0) {
    return (
      <div className="rounded-lg border border-hairline-soft bg-chip/40 px-2 py-1.5">
        {nameRow}
        <div className="mt-1.5 flex items-center gap-3.5">
          {parts.map((p) => (
            <span key={p.key} className="flex items-center gap-1.5">
              <MiniGauge
                ratio={p.pct! / 100}
                color={p.pct! <= 20 ? "var(--color-warn)" : EP_COLOR}
                size={22}
                stroke={2.2}
                icon={<span className="text-[8px] leading-none font-medium">{p.label}</span>}
                ariaLabel={t("peripherals.partAria", { name: d.name, part: p.label, pct: p.pct! })}
              />
              <span className="font-mono text-[10px] leading-none tabular-nums text-ink-dim">
                {p.pct}%
              </span>
            </span>
          ))}
        </div>
      </div>
    );
  }

  if (d.batteryMain != null) {
    return (
      <div className="flex items-center gap-2">
        <span className="w-[92px] min-w-0 shrink-0 truncate text-[9.5px] text-ink-dim" title={d.name}>
          {d.name}
        </span>
        <span className="min-w-0 flex-1">
          <MeterBar
            value={d.batteryMain}
            max={100}
            height={3}
            tone={d.batteryMain <= 20 ? "warn" : "accent"}
            label={t("peripherals.partAria", { name: d.name, part: d.deviceType, pct: d.batteryMain })}
          />
        </span>
        <span className="w-[32px] shrink-0 text-right font-mono text-[10px] leading-none tabular-nums text-ink-dim">
          {d.batteryMain}%
        </span>
      </div>
    );
  }

  return nameRow;
}

// ---------------------------------------------------------------- USB

function UsbCard({ devices }: { devices: USBDevice[] }) {
  const { t } = useI18n();
  return (
    <SectionCard
      title={t("peripherals.usbTitle")}
      tip={t("peripherals.usbTip")}
      icon={<UsbIcon size={13} />}
    >
      {devices.map((d, i) => {
        const meta = [d.vendor, d.speed, d.powerMa != null ? `${d.powerMa}mA` : null]
          .filter(Boolean)
          .join(" · ");
        return (
          <div key={`${d.name}:${i}`} className="flex min-w-0 flex-col">
            <span className="min-w-0 truncate text-[10.5px] text-ink-dim" title={d.name}>
              {d.name}
            </span>
            {meta ? (
              <span className="min-w-0 truncate text-[9px] text-ink-faint" title={meta}>
                {meta}
              </span>
            ) : null}
          </div>
        );
      })}
    </SectionCard>
  );
}

// ---------------------------------------------------------------- 音频

function AudioCard({ devices }: { devices: AudioDevice[] }) {
  const { t } = useI18n();
  const outputs = devices.filter((d) => d.direction === "output");
  const inputs = devices.filter((d) => d.direction === "input");

  return (
    <SectionCard
      title={t("peripherals.audioTitle")}
      tip={t("peripherals.audioTip")}
      icon={<HeadphonesIcon size={13} />}
    >
      <AudioGroup label={t("peripherals.output")} devices={outputs} />
      <AudioGroup label={t("peripherals.input")} devices={inputs} />
    </SectionCard>
  );
}

function AudioGroup({ label, devices }: { label: string; devices: AudioDevice[] }) {
  const { t } = useI18n();
  if (devices.length === 0) return null;
  return (
    <div className="flex min-w-0 flex-col gap-[5px]">
      <span className="text-[9px] text-ink-faint">{label}</span>
      {devices.map((d, i) => (
        <div key={`${d.name}:${i}`} className="flex min-w-0 items-center gap-2">
          <span className="min-w-0 flex-1 truncate text-[10.5px] text-ink-dim" title={d.name}>
            {d.name}
          </span>
          {d.isDefault ? (
            <Badge tone="accent" title={t("peripherals.audioTip")}>
              {t("peripherals.defaultDevice")}
            </Badge>
          ) : null}
        </div>
      ))}
    </div>
  );
}
