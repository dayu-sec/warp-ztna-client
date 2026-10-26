import { invoke } from "@tauri-apps/api/core";
import type { AccessRequest, AccessRequestCatalog, GroupGrant } from "./access";

export type ConnState =
  | "unknown"
  | "disconnected"
  | "connecting"
  | "connected"
  | "needs_login"
  | "error";

export type StreamHealth = "live" | "reconnecting" | "unavailable";

export type AgentStatus = {
  state: ConnState;
  peer_ip?: string | null;
  hostname?: string | null;
  detail?: string | null;
  daemon_version?: string | null;
  session_expires_at?: number | null;
  routes: string[];
  link_ok: boolean;
  stream: StreamHealth;
};

export type ApiProbe = {
  status: "ok" | "unreachable" | "unconfigured";
  detail?: string | null;
};

export type SsoStart =
  | { kind: "ready" }
  | {
      kind: "sso_required";
      user_code: string;
      verification_uri: string;
      verification_uri_complete: string;
    };

export type ExtendStart = {
  user_code: string;
  device_code: string;
  verification_uri: string;
  verification_uri_complete: string;
  expires_in: number;
};

export type ProfileRow = {
  id: string;
  name: string;
  url: string;
  is_active: boolean;
};

export type AccessResourcePayload = {
  requests: AccessRequest[];
  catalog: AccessRequestCatalog;
  group_grants: GroupGrant[];
};

export type MeshStatus = {
  status: "approved" | "pending" | "rejected" | "not_enrolled" | "unknown";
  email?: string | null;
  rejection_reason?: string | null;
  rejected_at?: string | null;
};

export type CurrentUser = {
  email?: string | null;
};

export type NativePrefs = {
  lang: "zh" | "en";
  notify: boolean;
  sessionLeadMin: number;
  trayTheme: "dark" | "light";
};

export type TrayMenuLabels = {
  open: string;
  request: string;
  resources: string;
  connect: string;
  disconnect: string;
  settings: string;
  quit: string;
  more: string;
};

export type TrayResource = {
  label: string;
  status: string;
};

export type SshLaunchPlan = {
  status: "launched" | "copy_only";
  platform: "macos" | "windows" | "linux" | "other";
  host: string;
  account: string;
  command: string;
  launch_error?: string | null;
  key_path: string;
  certificate_path: string;
  certificate_serial?: string | null;
  certificate_valid_before?: string | null;
  known_hosts_path: string;
  known_hosts_action: "appended" | "already_trusted" | "kept_other_pattern";
  known_hosts_pattern?: string | null;
  terminal_available: boolean;
};

export type SshPreparePlan = {
  command: string;
  key_path: string;
  certificate_path: string;
};

export const PENDING_APPROVAL_CODE = "biz.pending_approval";

export const PEER_CONFLICT_CODE = "biz.peer_ownership_conflict";

export const SSH_NO_LOCAL_KEY_CODE = "biz.ssh_no_local_key";

export function isPendingApprovalError(cause: unknown): boolean {
  return Boolean(
    cause &&
      typeof cause === "object" &&
      (cause as { code?: unknown }).code === PENDING_APPROVAL_CODE,
  );
}

export function isPeerConflictError(cause: unknown): boolean {
  return Boolean(
    cause &&
      typeof cause === "object" &&
      (cause as { code?: unknown }).code === PEER_CONFLICT_CODE,
  );
}

export function isNoLocalKeyError(cause: unknown): boolean {
  return Boolean(
    cause &&
      typeof cause === "object" &&
      (cause as { code?: unknown }).code === SSH_NO_LOCAL_KEY_CODE,
  );
}

export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export function formatInvokeError(cause: unknown): string {
  if (typeof cause === "string" && cause.trim()) return cause;
  if (cause && typeof cause === "object") {
    const record = cause as { message?: unknown; code?: unknown };
    if (typeof record.message === "string" && record.message.trim()) {
      return record.message;
    }
  }
  if (cause instanceof Error && cause.message.trim()) return cause.message;
  return "";
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(cmd, args);
  } catch (cause) {
    const error = new Error(formatInvokeError(cause) || "request failed") as Error & {
      code?: string;
    };
    if (cause && typeof cause === "object" && "code" in cause) {
      const code = (cause as { code?: unknown }).code;
      if (typeof code === "string") error.code = code;
    }
    throw error;
  }
}

export type ServiceConfig = {
  stored_api_url: string;
  effective_api_url: string;
};

export type FrontendAction = "settings" | "connect";

export const native = {
  status: () => call<AgentStatus>("agent_status"),
  apiProbe: () => call<ApiProbe>("api_probe"),
  up: () => call<AgentStatus>("up"),
  disconnect: () => call<AgentStatus>("disconnect"),
  startSso: () => call<SsoStart>("start_sso"),
  checkLogin: () => call<SsoStart>("check_login"),
  waitSso: (userCode: string) => call<AgentStatus>("wait_sso", { userCode }),
  extendSso: () => call<ExtendStart>("extend_sso"),
  waitExtendSso: (deviceCode: string, userCode: string) =>
    call<AgentStatus>("wait_extend_sso", { deviceCode, userCode }),
  dismissSessionWarning: () => call<void>("dismiss_session_warning"),
  listProfiles: () => call<ProfileRow[]>("list_profiles"),
  addProfile: (name: string, url: string) => call<string>("add_profile", { name, url }),
  updateProfile: (handle: string, name: string, url: string) =>
    call<void>("update_profile", { handle, name, url }),
  removeProfile: (handle: string) => call<void>("remove_profile", { handle }),
  switchProfile: (handle: string) => call<string>("switch_profile", { handle }),
  myAccessResources: () => call<AccessResourcePayload>("my_access_resources"),
  myMeshStatus: () => call<MeshStatus>("my_mesh_status"),
  currentUser: () => call<CurrentUser>("current_user"),
  acquireApiSession: (focus = false) => call<void>("acquire_api_session", { focus }),
  cancelApiSession: () => call<void>("cancel_api_session"),
  clearApiSession: () => call<void>("clear_api_session"),
  setTrayIconTheme: (theme: "dark" | "light") =>
    call<void>("set_tray_icon_theme", { theme }),
  setTrayMenu: (
    labels: TrayMenuLabels,
    connected: boolean,
    resources: TrayResource[],
    more: boolean,
    apiBlocked: boolean,
  ) => call<void>("set_tray_menu", { labels, connected, resources, more, apiBlocked }),
  setWindowSize: async (width: number, height: number) => {
    if (!isTauri()) return;
    const { getCurrentWindow, LogicalSize } = await import("@tauri-apps/api/window");
    await getCurrentWindow().setSize(new LogicalSize(width, height));
  },
  showWindow: async () => {
    if (!isTauri()) return;
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    await getCurrentWindow().show();
  },
  openUrl: (url: string) => call<void>("open_url", { url }),
  openPortal: (view?: string) => call<void>("open_portal", { view: view ?? null }),
  openHelp: () => call<void>("open_help"),
  sshLaunch: (requestId: string, address: string) =>
    call<SshLaunchPlan>("ssh_launch", { requestId, address }),
  sshPrepare: (requestId: string, address: string) =>
    call<SshPreparePlan>("ssh_prepare", { requestId, address }),
  quitApp: () => call<void>("quit_app"),
  getServiceConfig: () => call<ServiceConfig>("get_service_config"),
  saveServiceConfig: (apiUrl: string) => call<void>("save_service_config", { apiUrl }),
  frontendReady: () => call<FrontendAction | null>("frontend_ready"),
  getNativePrefs: () => call<NativePrefs | null>("get_native_prefs"),
  setNativePrefs: (prefs: NativePrefs) => call<void>("set_native_prefs", { prefs }),
  restartApp: () => call<void>("restart_app"),
  startDragging: async () => {
    if (!isTauri()) return;
    try {
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      await getCurrentWindow().startDragging();
    } catch {
      await call<void>("start_dragging").catch(() => undefined);
    }
  },
  minimize: async () => {
    if (!isTauri()) return;
    try {
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      await getCurrentWindow().minimize();
    } catch {
      await call<void>("minimize_window").catch(() => undefined);
    }
  },
  toggleMaximize: async () => {
    if (!isTauri()) return;
    try {
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      await getCurrentWindow().toggleMaximize();
    } catch {
      await call<void>("toggle_maximize_window").catch(() => undefined);
    }
  },
  setAutostart: async (enabled: boolean) => {
    if (!isTauri()) return;
    const { enable, disable } = await import("@tauri-apps/plugin-autostart");
    if (enabled) await enable();
    else await disable();
  },
  requestNotifyPermission: async (): Promise<boolean> => {
    if (!isTauri()) return false;
    const { isPermissionGranted, requestPermission } = await import(
      "@tauri-apps/plugin-notification"
    );
    if (await isPermissionGranted()) return true;
    return (await requestPermission()) === "granted";
  },
  notify: async (title: string, body?: string) => {
    if (!isTauri()) return;
    try {
      const { sendNotification } = await import("@tauri-apps/plugin-notification");
      if (!(await native.requestNotifyPermission())) return;
      sendNotification({ title, body: body ?? "" });
    } catch {
      /* OS notification is best-effort */
    }
  },
  appVersion: async () => {
    if (!isTauri()) return "";
    const { getVersion } = await import("@tauri-apps/api/app");
    return getVersion();
  },
  watchWindowVisibility: async (onChange: (visible: boolean) => void) => {
    if (!isTauri()) return () => undefined;
    const { listen } = await import("@tauri-apps/api/event");
    return listen<boolean>("window-visibility", (event) => onChange(Boolean(event.payload)));
  },
  watchAgentStatus: async (onStatus: (status: AgentStatus) => void) => {
    if (!isTauri()) return () => undefined;
    const { listen } = await import("@tauri-apps/api/event");
    return listen<AgentStatus>("agent-status", (event) => onStatus(event.payload));
  },
  watchTrayConnect: async (onConnect: () => void) => {
    if (!isTauri()) return () => undefined;
    const { listen } = await import("@tauri-apps/api/event");
    return listen("tray-connect", () => onConnect());
  },
  watchMaximized: async (onChange: (maximized: boolean) => void) => {
    if (!isTauri()) return () => undefined;
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    const win = getCurrentWindow();
    onChange(await win.isMaximized());
    const unlisten = await win.onResized(async () => {
      onChange(await win.isMaximized());
    });
    return unlisten;
  },
};
