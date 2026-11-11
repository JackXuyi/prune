/**
 * 中文 catalog —— **唯一 source of truth**。
 *
 * 所有 `MessageKey` / 参数类型都从这里派生（见 ../types.ts），
 * 因此新增文案必须先写中文；英文侧由 `MessageTree<typeof zh>` 强制补齐。
 */
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

export const zh = {
  chart: chart.zh,
  common: common.zh,
  disk: disk.zh,
  island: island.zh,
  lan: lan.zh,
  memory: memory.zh,
  nav: nav.zh,
  net: net.zh,
  panel: panel.zh,
  ports: ports.zh,
  resources: resources.zh,
  settings: settings.zh,
  stats: stats.zh,
  update: update.zh,
} as const;
