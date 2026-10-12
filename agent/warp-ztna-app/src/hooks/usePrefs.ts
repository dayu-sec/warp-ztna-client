import { useCallback, useEffect, useState } from "react";
import { isTauri, native } from "../api/native";
import { t, type Lang } from "../i18n";
import { resolvedTheme } from "../lib/platform";
import type { Prefs, ThemePref } from "../types/ui";

export function usePrefs() {
  const [lang, setLang] = useState<Lang>(() => (localStorage.getItem("warp-ztna-lang") as Lang) || "zh");
  const [themePref, setThemePref] = useState<ThemePref>(
    () => (localStorage.getItem("warp-ztna-theme") as ThemePref) || "dark",
  );
  const [systemTheme, setSystemTheme] = useState<"dark" | "light">(() =>
    window.matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark",
  );
  const [prefs, setPrefs] = useState<Prefs>({
    notify: localStorage.getItem("warp-ztna-opt-notify") !== "0",
    startup: localStorage.getItem("warp-ztna-opt-startup") === "1",
    launch: localStorage.getItem("warp-ztna-opt-launch") !== "0",
    sessionWarningLeadMin: readLeadMin(),
    assetRefreshMin: readAssetRefreshMin(),
  });

  const theme: "dark" | "light" = themePref === "system" ? systemTheme : themePref;
  const tr = useCallback((key: string, vars?: Record<string, string>) => t(lang, key, vars), [lang]);

  useEffect(() => {
    localStorage.setItem("warp-ztna-lang", lang);
    document.title = `${tr("app.name")} · Warp-ZTNA`;
  }, [lang, tr]);

  useEffect(() => {
    localStorage.setItem("warp-ztna-theme", themePref);
  }, [themePref]);

  useEffect(() => {
    const query = window.matchMedia("(prefers-color-scheme: light)");
    const onChange = (event: MediaQueryListEvent) => {
      setSystemTheme(event.matches ? "light" : "dark");
    };
    setSystemTheme(query.matches ? "light" : "dark");
    query.addEventListener("change", onChange);
    return () => query.removeEventListener("change", onChange);
  }, []);

  useEffect(() => {
    if (!isTauri()) return;
    void native.setAutostart(prefs.launch).catch(() => undefined);
    // Apply once at boot so the OS login item matches the saved switch.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  function toggleTheme() {
    setThemePref(resolvedTheme(themePref) === "dark" ? "light" : "dark");
  }

  function updatePref(key: "notify" | "startup" | "launch", value: boolean, storageKey: string) {
    setPrefs((prev) => ({ ...prev, [key]: value }));
    localStorage.setItem(storageKey, value ? "1" : "0");
    if (!isTauri()) return;
    if (key === "launch") {
      void native.setAutostart(value).catch(() => undefined);
    }
    if (key === "notify" && value) {
      void native.requestNotifyPermission();
    }
  }

  function setSessionWarningLeadMin(minutes: number) {
    const next = clampLeadMin(minutes);
    setPrefs((prev) => ({ ...prev, sessionWarningLeadMin: next }));
    localStorage.setItem("warp-ztna-opt-session-lead-min", String(next));
  }

  function setAssetRefreshMin(minutes: number) {
    const next = clampAssetRefreshMin(minutes);
    setPrefs((prev) => ({ ...prev, assetRefreshMin: next }));
    localStorage.setItem("warp-ztna-opt-asset-refresh-min", String(next));
  }

  return {
    lang,
    setLang,
    themePref,
    setThemePref,
    systemTheme,
    theme,
    prefs,
    updatePref,
    setSessionWarningLeadMin,
    setAssetRefreshMin,
    toggleTheme,
    tr,
  };
}

function readAssetRefreshMin(): number {
  const raw = localStorage.getItem("warp-ztna-opt-asset-refresh-min");
  const parsed = raw ? Number.parseInt(raw, 10) : Number.NaN;
  return clampAssetRefreshMin(Number.isFinite(parsed) ? parsed : 5);
}

function clampAssetRefreshMin(minutes: number): number {
  if (!Number.isFinite(minutes)) return 5;
  return Math.min(60, Math.max(0, Math.round(minutes)));
}

function readLeadMin(): number {
  const raw = localStorage.getItem("warp-ztna-opt-session-lead-min");
  const parsed = raw ? Number.parseInt(raw, 10) : Number.NaN;
  return clampLeadMin(Number.isFinite(parsed) ? parsed : 15);
}

function clampLeadMin(minutes: number): number {
  if (!Number.isFinite(minutes)) return 15;
  return Math.min(59, Math.max(1, Math.round(minutes)));
}
