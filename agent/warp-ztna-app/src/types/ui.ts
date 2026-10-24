export type UiConn =
  | "needs-login"
  | "disconnected"
  | "connecting"
  | "reconnecting"
  | "connected"
  | "error";
export type ThemePref = "dark" | "light" | "system";
export type Surface = "main" | "settings";
export type SettingsTab = "general" | "profiles" | "about";
export type Platform = "mac" | "win" | "linux";


export type Prefs = {
  notify: boolean;
  launch: boolean;
  sessionWarningLeadMin: number;
  assetRefreshMin: number;
};
