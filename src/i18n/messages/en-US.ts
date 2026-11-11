/**
 * 英文 catalog。`satisfies Messages` 是硬闸：
 * 少一个 key、多一个 key、值不是 string —— `tsc` 直接失败。
 *
 * 各 namespace 文件内部已经用 `MessageTree<typeof zh>` 校验过一次；
 * 这里的 `satisfies` 是第二道：防止聚合层漏挂 namespace。
 */
import type { Messages } from "../types";
import { chart } from "./namespaces/chart";
import { common } from "./namespaces/common";
import { disk } from "./namespaces/disk";
import { island } from "./namespaces/island";
import { lan } from "./namespaces/lan";
import { memory } from "./namespaces/memory";
import { nav } from "./namespaces/nav";
import { net } from "./namespaces/net";
import { panel } from "./namespaces/panel";
import { ports } from "./namespaces/ports";
import { resources } from "./namespaces/resources";
import { settings } from "./namespaces/settings";
import { stats } from "./namespaces/stats";
import { update } from "./namespaces/update";

export const en = {
  chart: chart.en,
  common: common.en,
  disk: disk.en,
  island: island.en,
  lan: lan.en,
  memory: memory.en,
  nav: nav.en,
  net: net.en,
  panel: panel.en,
  ports: ports.en,
  resources: resources.en,
  settings: settings.en,
  stats: stats.en,
  update: update.en,
} as const satisfies Messages;
