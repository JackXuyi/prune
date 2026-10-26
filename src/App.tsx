import { useEffect, useState } from "react";

import { getCurrentWindow } from "@tauri-apps/api/window";

import { isTauri } from "./lib/ipc";
import { MenuBarPanel } from "./panels/MenuBarPanel";
import { useDiskStore } from "./stores/diskStore";
import { useMemStore } from "./stores/memStore";
import { MainWindow } from "./windows/MainWindow";

/** 两个窗口共用同一个 index.html，靠窗口 label 分流；浏览器里可用 ?window=panel 强制 */
function detectWindowLabel(): string {
  if (typeof window !== "undefined") {
    const forced = new URLSearchParams(window.location.search).get("window");
    if (forced === "panel" || forced === "main") return forced;
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
    void init();
    void initDisk();
  }, [init, initDisk]);

  return label === "panel" ? <MenuBarPanel /> : <MainWindow />;
}
