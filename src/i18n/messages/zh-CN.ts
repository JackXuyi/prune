/**
 * 中文 catalog —— **唯一 source of truth**。
 *
 * 所有 `MessageKey` / 参数类型都从这里派生（见 ../types.ts），
 * 因此新增文案必须先写中文；英文侧由 `MessageTree<typeof zh>` 强制补齐。
 */
import { common } from "./namespaces/common";
import { nav } from "./namespaces/nav";
import { settings } from "./namespaces/settings";
import { stats } from "./namespaces/stats";
import { update } from "./namespaces/update";

export const zh = {
  common: common.zh,
  nav: nav.zh,
  settings: settings.zh,
  stats: stats.zh,
  update: update.zh,
} as const;
