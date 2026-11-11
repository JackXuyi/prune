import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import App from "./App";
import { useLocaleStore } from "./i18n/store";
import "./index.css";

/**
 * 先 resolve 语言，再 render（docs/i18n-spec.md §2.4）。
 *
 * 反过来做的话，首帧用默认语言画一遍、拿到 locale 后再画一遍 —— 中文用户会看到
 * 「英文闪一下变中文」。`init()` 内部幂等，StrictMode 双调用无害。
 */
async function boot() {
  await useLocaleStore.getState().init();
  const { locale } = useLocaleStore.getState();
  document.documentElement.lang = locale === "zh" ? "zh-Hans" : "en";
  createRoot(document.getElementById("root")!).render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
}

void boot();
