import { useEffect, useState } from "react";

import { getCurrentWindow } from "@tauri-apps/api/window";

import { isTauri } from "./lib/ipc";
import { Island } from "./panels/Island";
import { MenuBarPanel } from "./panels/MenuBarPanel";
import { useDiskStore } from "./stores/diskStore";
import { useMemStore } from "./stores/memStore";
import { MainWindow } from "./windows/MainWindow";

/** 三个窗口共用同一个 index.html，靠窗口 label 分流；浏览器里可用 ?window=panel 强制 */
function detectWindowLabel(): string {
  if (typeof window !== "undefined") {
    const forced = new URLSearchParams(window.location.search).get("window");
    if (forced === "panel" || forced === "main" || forced === "island") return forced;
  }
  if (!isTauri()) return "main";
  try {
    return getCurrentWindow().label;
  } catch {
    return "main";
  }
}

export default function App() {
  const [label] = useState(detectWindowLabel);
  const init = useMemStore((s) => s.init);
  const initDisk = useDiskStore((s) => s.init);

  useEffect(() => {
    // 岛只渲染一次提报，不需要内存/磁盘快照 —— 少两条 IPC，也少一份常驻 store
    if (label === "island") return;
    void init();
    void initDisk();
  }, [label, init, initDisk]);

  if (label === "island") return <Island />;
  return label === "panel" ? <MenuBarPanel /> : <MainWindow />;
}
