"use client";

import { useEffect } from "react";
import {
  bindMiniAppCssVars,
  bindThemeParamsCssVars,
  bindViewportCssVars,
  init,
  miniAppReady,
  mountMiniAppSync,
  mountThemeParamsSync,
  mountViewport,
  unmountMiniApp,
  unmountThemeParams,
  unmountViewport,
} from "@telegram-apps/sdk-react";

export function TgInit() {
  useEffect(() => {
    const cleanups: Array<() => void> = [];

    cleanups.push(init({ acceptCustomStyles: true }));

    // Component mounts.
    if (mountMiniAppSync.isAvailable()) mountMiniAppSync();
    if (mountThemeParamsSync.isAvailable()) mountThemeParamsSync();
    if (mountViewport.isAvailable()) mountViewport();

    // Bind Telegram-provided CSS vars:
    // - theme params: --tg-theme-*
    // - viewport insets/sizes: --tg-viewport-*
    // - miniapp colors: --tg-mini-app-*
    if (bindThemeParamsCssVars.isAvailable())
      cleanups.push(bindThemeParamsCssVars());
    if (bindViewportCssVars.isAvailable()) cleanups.push(bindViewportCssVars());
    if (bindMiniAppCssVars.isAvailable()) cleanups.push(bindMiniAppCssVars());

    if (miniAppReady.isAvailable()) miniAppReady();

    return () => {
      for (let i = cleanups.length - 1; i >= 0; i -= 1) cleanups[i]?.();
      unmountViewport();
      unmountThemeParams();
      unmountMiniApp();
    };
  }, []);

  return null;
}
