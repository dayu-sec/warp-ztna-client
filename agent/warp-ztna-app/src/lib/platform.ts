import logoGreen from "../assets/logo-green-transparent.png";
import logoPink from "../assets/logo-pink-transparent.png";
import type { Platform, ThemePref } from "../types/ui";

export function detectPlatform(): Platform {
  const ua = navigator.userAgent || "";
  if (/Windows/i.test(ua)) return "win";
  if (/Linux/i.test(ua) && !/Android/i.test(ua)) return "linux";
  return "mac";
}

export function resolvedTheme(pref: ThemePref): "dark" | "light" {
  if (pref === "system") {
    return window.matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark";
  }
  return pref;
}

export function logoSrc(connected: boolean): string {
  return connected ? logoGreen : logoPink;
}
