import type { MouseEvent } from "react";
import { native } from "../api/native";
import type { Lang } from "../i18n";
import { t } from "../i18n";
import type { Platform, Surface } from "../types/ui";
import { IconBack, IconMore, MoonIcon, SunIcon } from "./Icons";

function isInteractiveDragTarget(target: EventTarget | null): boolean {
  return Boolean(
    target instanceof Element && target.closest("button, a, input, textarea, select, [role='menu'], [data-no-drag]"),
  );
}

function WindowControls({
  maximized,
  onMinimize,
  onMaximize,
  onClose,
}: {
  maximized: boolean;
  onMinimize: () => void;
  onMaximize: () => void;
  onClose: () => void;
}) {
  return (
    <div className="win-controls">
      <button type="button" className="win-ctrl min" aria-label="Minimize" onClick={onMinimize}>
        <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
          <path d="M1 5h8" stroke="currentColor" strokeWidth="1.2" />
        </svg>
      </button>
      <button type="button" className="win-ctrl max" aria-label="Maximize" onClick={onMaximize}>
        {maximized ? (
          <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
            <path d="M3 3.6h4.2V8H3zM2.4 2h4.8v1.2H3.6V7.2H2.4z" fill="currentColor" />
          </svg>
        ) : (
          <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
            <rect x="1.4" y="1.4" width="7.2" height="7.2" fill="none" stroke="currentColor" strokeWidth="1.2" />
          </svg>
        )}
      </button>
      <button type="button" className="win-ctrl close" aria-label="Close" onClick={onClose}>
        <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
          <path d="M2 2l6 6M8 2L2 8" stroke="currentColor" strokeWidth="1.2" />
        </svg>
      </button>
    </div>
  );
}

export function Titlebar({
  lang,
  theme,
  surface,
  platform,
  maximized,
  updateAvailable,
  onBack,
  onOpenSettings,
  onToggleTheme,
}: {
  lang: Lang;
  theme: "dark" | "light";
  surface: Surface;
  platform: Platform;
  maximized: boolean;
  updateAvailable: boolean;
  onBack: () => void;
  onOpenSettings: () => void;
  onToggleTheme: () => void;
}) {
  function onWindowDrag(e: MouseEvent) {
    if (e.button !== 0 || isInteractiveDragTarget(e.target)) return;
    void native.startDragging();
  }

  return (
    <header className="titlebar" data-tauri-drag-region onMouseDown={onWindowDrag}>
      <div className="titlebar-left">
        {surface === "settings" && (
          <button type="button" className="tb-btn titlebar-back" onClick={onBack}>
            <IconBack />
            <span>{t(lang, "action.backMain")}</span>
          </button>
        )}
      </div>
      <div className="titlebar-drag" data-tauri-drag-region />
      <div className="titlebar-right">
        <button
          type="button"
          className="tb-btn icon theme-btn"
          aria-label={t(lang, "nav.appearance")}
          title={t(lang, "nav.appearance")}
          onClick={onToggleTheme}
        >
          {theme === "dark" ? <MoonIcon /> : <SunIcon />}
        </button>
        <button
          type="button"
          className="tb-btn icon more-btn"
          aria-label={t(lang, updateAvailable ? "nav.settingsUpdate" : "nav.settings")}
          title={t(lang, updateAvailable ? "nav.settingsUpdate" : "nav.settings")}
          onClick={onOpenSettings}
        >
          <IconMore />
          {updateAvailable && <span className="tb-update-dot" aria-hidden="true" />}
        </button>
        {platform !== "mac" && (
          <WindowControls
            maximized={maximized}
            onMinimize={() => void native.minimize()}
            onMaximize={() => void native.toggleMaximize()}
            onClose={() => void native.quitApp()}
          />
        )}
      </div>
    </header>
  );
}
