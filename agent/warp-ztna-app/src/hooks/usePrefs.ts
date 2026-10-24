import { useCallback, useEffect, useRef, useState } from "react";
import { isTauri, native, type NativePrefs } from "../api/native";
import { t, type Lang } from "../i18n";
import { resolvedTheme } from "../lib/platform";
import type { Prefs, ThemePref } from "../types/ui";

export function usePrefs() {
  const [lang, setLangState] = useState<Lang>(() => (localStorage.getItem("warp-ztna-lang") as Lang) || "zh");
  const [themePref, setThemePrefState] = useState<ThemePref>(
    () => (localStorage.getItem("warp-ztna-theme") as ThemePref) || "dark",
  );
  const [systemTheme, setSystemTheme] = useState<"dark" | "light">(() =>
    window.matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark",
  );
  const [prefs, setPrefs] = useState<Prefs>({
    notify: localStorage.getItem("warp-ztna-opt-notify") !== "0",
    launch: localStorage.getItem("warp-ztna-opt-launch") !== "0",
    sessionWarningLeadMin: readLeadMin(),
    assetRefreshMin: readAssetRefreshMin(),
  });
  const hydratedRef = useRef(false);

  const theme: "dark" | "light" = themePref === "system" ? systemTheme : themePref;
  const tr = useCallback((key: string, vars?: Record<string, string>) => t(lang, key, vars), [lang]);

  function syncNative(snapshot: NativePrefs) {
    if (!isTauri() || !hydratedRef.current) return;
    void native.setNativePrefs(snapshot).catch(() => undefined);
  }

  function snapshot(overrides: Partial<NativePrefs> = {}): NativePrefs {
    return {
      lang,
      notify: prefs.notify,
      sessionLeadMin: prefs.sessionWarningLeadMin,
      trayTheme: systemTheme,
      ...overrides,
    };
  }

  useEffect(() => {
    if (!isTauri()) return;
    let cancelled = false;
    void native
      .getNativePrefs()
      .then((stored) => {
        if (cancelled) return;
        if (!stored) {
          void native.setNativePrefs(snapshot()).catch(() => undefined);
          hydratedRef.current = true;
          return;
        }
        const lead = clampLeadMin(stored.sessionLeadMin);
        setLangState(stored.lang);
        setPrefs((prev) => ({
          ...prev,
          notify: stored.notify,
          sessionWarningLeadMin: lead,
        }));
        localStorage.setItem("warp-ztna-lang", stored.lang);
        localStorage.setItem("warp-ztna-opt-notify", stored.notify ? "1" : "0");
        localStorage.setItem("warp-ztna-opt-session-lead-min", String(lead));
        hydratedRef.current = true;
        syncNative({ ...stored, sessionLeadMin: lead, trayTheme: systemTheme });
      })
      .catch(() => {
        hydratedRef.current = true;
      });
    return () => {
      cancelled = true;
    };
    // Hydrate once from the native mirror; later changes write through the setters.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

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
    syncNative(snapshot({ trayTheme: systemTheme }));
    // Only the OS appearance changes the tray theme; other setters sync themselves.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [systemTheme]);

  useEffect(() => {
    if (!isTauri()) return;
    void native.setAutostart(prefs.launch).catch(() => undefined);
    // Apply once at boot so the OS login item matches the saved switch.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  function setLang(next: Lang) {
    setLangState(next);
    syncNative(snapshot({ lang: next }));
  }

  function setThemePref(next: ThemePref) {
    setThemePrefState(next);
  }

  function toggleTheme() {
    setThemePrefState(resolvedTheme(themePref) === "dark" ? "light" : "dark");
  }

  function updatePref(key: "notify" | "launch", value: boolean, storageKey: string) {
    const next = { ...prefs, [key]: value };
    setPrefs(next);
    localStorage.setItem(storageKey, value ? "1" : "0");
    syncNative({
      lang,
      notify: next.notify,
      sessionLeadMin: next.sessionWarningLeadMin,
      trayTheme: systemTheme,
    });
    if (!isTauri()) return;
    if (key === "launch") {
      void native.setAutostart(value).catch(() => undefined);
    }
    if (key === "notify" && value) {
      void native.requestNotifyPermission();
    }
  }

  function setSessionWarningLeadMin(minutes: number) {
    const next = { ...prefs, sessionWarningLeadMin: clampLeadMin(minutes) };
    setPrefs(next);
    localStorage.setItem("warp-ztna-opt-session-lead-min", String(next.sessionWarningLeadMin));
    syncNative({
      lang,
      notify: next.notify,
      sessionLeadMin: next.sessionWarningLeadMin,
      trayTheme: systemTheme,
    });
  }

  function setAssetRefreshMin(minutes: number) {
    const next = { ...prefs, assetRefreshMin: clampAssetRefreshMin(minutes) };
    setPrefs(next);
    localStorage.setItem("warp-ztna-opt-asset-refresh-min", String(next.assetRefreshMin));
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
