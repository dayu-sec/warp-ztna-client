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

function CloseControl({ onClose }: { onClose: () => void }) {
  return (
    <div className="win-controls">
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
  updateAvailable,
  onBack,
  onOpenSettings,
  onToggleTheme,
}: {
  lang: Lang;
  theme: "dark" | "light";
  surface: Surface;
  platform: Platform;
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
          <CloseControl
            onClose={() => {
              void native
                .closeWindow()
                .catch((cause) => console.error("close window failed", cause));
            }}
          />
        )}
      </div>
    </header>
  );
}
