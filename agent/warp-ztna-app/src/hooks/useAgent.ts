import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { writeText as writeTauriClipboard } from "@tauri-apps/plugin-clipboard-manager";
import {
  addressKey,
  collectRows,
  mapAccessResources,
  type AccessRequest,
  type AccessRequestCatalog,
  type GroupGrant,
  type ResourceRow,
} from "../api/access";
import {
  formatInvokeError,
  isNoLocalKeyError,
  isPeerConflictError,
  isPendingApprovalError,
  isTauri,
  native,
  type AgentStatus,
  type ApiProbe,
  type SshLaunchPlan,
} from "../api/native";
import { nextConnection } from "../lib/connState";
import { isDaemonDown, isDaemonLive } from "../lib/daemonDown";
import {
  describeFlips,
  livenessFlips,
  livenessState,
  type LivenessState,
} from "../lib/resourceLiveness";
import { ssoSignInUrl } from "../lib/ssoUrl";
import type { UiConn } from "../types/ui";

const CONNECT_BURST_MS = [1_500, 4_000, 8_000];

const CONNECT_GRACE_MS = 15_000;

const CONNECT_POLL_MS = 5_000;

const API_PROBE_RETRY_MS = 5_000;

async function writeToClipboard(text: string): Promise<boolean> {
  if (!text) return false;
  try {
    if (isTauri()) {
      await writeTauriClipboard(text);
    } else if (navigator.clipboard) {
      await navigator.clipboard.writeText(text);
    } else {
      return false;
    }
    return true;
  } catch {
    return false;
  }
}

export function useAgent(
  tr: (key: string, vars?: Record<string, string>) => string,
  showToast: (msg: string) => void,
  loadProfiles: () => Promise<void>,
  warningLeadMin = 15,
  assetRefreshMin = 5,
  notifyEnabled = false,
) {
  const [connection, setConnection] = useState<UiConn>("needs-login");
  const [hostname, setHostname] = useState("—");
  const [userEmail, setUserEmail] = useState("—");
  const [overlayIp, setOverlayIp] = useState("");
  const [linkOk, setLinkOk] = useState(false);
  const [daemonVersion, setDaemonVersion] = useState("0.76.x");
  const [routes, setRoutes] = useState<string[]>([]);
  const [grantRequests, setGrantRequests] = useState<AccessRequest[]>([]);
  const [grantCatalog, setGrantCatalog] = useState<AccessRequestCatalog>({ groups: [], addresses: [] });
  const [groupGrants, setGroupGrants] = useState<GroupGrant[]>([]);
  const [resourcesBusy, setResourcesBusy] = useState(false);
  const [resourcesOkRun, setResourcesOkRun] = useState(0);
  const [ssoBusy, setSsoBusy] = useState(false);
  const [ssoUrl, setSsoUrl] = useState("");
  const [pendingApproval, setPendingApproval] = useState(false);
  const [peerConflict, setPeerConflict] = useState(false);
  const [meshRejected, setMeshRejected] = useState(false);
  const [sessionExpired, setSessionExpired] = useState(false);
  const [sessionExpiring, setSessionExpiring] = useState(false);
  const [copied, setCopied] = useState(false);
  const [sshBusyKey, setSshBusyKey] = useState<string | null>(null);
  const [sshFallback, setSshFallback] = useState<SshLaunchPlan | null>(null);
  const [windowHidden, setWindowHidden] = useState(false);
  const [mainGate, setMainGate] = useState<"pending" | "welcome" | "home">("pending");
  const [daemonDown, setDaemonDown] = useState(false);
  const [apiBlocked, setApiBlocked] = useState<ApiProbe | null>(null);
  const [probeRun, setProbeRun] = useState(0);
  const announceProbeRef = useRef(false);
  const warningLeadSecs = Math.max(1, warningLeadMin) * 60;
  const sessionDismissedFor = useRef<number | null>(null);
  const ssoRunRef = useRef(0);
  const ssoInFlightRef = useRef(false);
  const meshRetryAtRef = useRef(0);
  const ssoActionRef = useRef<"login" | "extend">("login");
  const prevConnection = useRef<UiConn>(connection);
  const notifyEnabledRef = useRef(notifyEnabled);
  notifyEnabledRef.current = notifyEnabled;
  const assetsRunRef = useRef(0);

  const connected = connection === "connected";
  const connecting = connection === "connecting";
  const signedIn = connection !== "needs-login";
  const connectionRef = useRef(connection);
  connectionRef.current = connection;
  const mainGateRef = useRef(mainGate);
  mainGateRef.current = mainGate;
  const windowHiddenRef = useRef(windowHidden);
  const showToastRef = useRef(showToast);
  showToastRef.current = showToast;
  const trRef = useRef(tr);
  trRef.current = tr;
  const startSsoRef = useRef(startSso);
  startSsoRef.current = startSso;
  const connectUntilRef = useRef(0);
  const beginConnectWindow = useCallback(() => {
    connectUntilRef.current = Date.now() + CONNECT_GRACE_MS;
  }, []);
  const endConnectWindow = useCallback(() => {
    connectUntilRef.current = 0;
  }, []);

  const applyStatus = useCallback((status: AgentStatus, signed: boolean, leadSecs = warningLeadSecs) => {
    const connectPending = Date.now() < connectUntilRef.current;
    const settled =
      status.state === "connected" ||
      status.state === "needs_login" ||
      status.state === "error";
    const idle = status.state === "disconnected" || status.state === "unknown";
    if (settled || (idle && !connectPending)) endConnectWindow();
    setConnection((prev) => {
      const next = nextConnection(prev, status, signed, connectPending);
      return prev === next ? prev : next;
    });
    if (status.hostname) {
      const nextHost = status.hostname;
      setHostname((prev) => (prev === nextHost ? prev : nextHost));
    }
    setOverlayIp((prev) => {
      const next = status.peer_ip ?? "";
      return prev === next ? prev : next;
    });
    setLinkOk((prev) => (prev === status.link_ok ? prev : status.link_ok));
    if (status.daemon_version) {
      const nextVer = status.daemon_version;
      setDaemonVersion((prev) => (prev === nextVer ? prev : nextVer));
    }
    setRoutes((prev) => {
      const next = status.routes ?? [];
      return prev.length === next.length && prev.every((item, i) => item === next[i]) ? prev : next;
    });
    if (status.session_expires_at) {
      const remain = status.session_expires_at - Math.floor(Date.now() / 1000);
      const withinLead = remain > 0 && remain < leadSecs;
      const dismissed = sessionDismissedFor.current === status.session_expires_at;
      const nextExpiring = withinLead && !dismissed;
      setSessionExpiring((prev) => (prev === nextExpiring ? prev : nextExpiring));
    } else {
      setSessionExpiring((prev) => (prev ? false : prev));
    }
  }, [warningLeadSecs, endConnectWindow]);
  const applyStatusRef = useRef(applyStatus);
  applyStatusRef.current = applyStatus;

  const loadResources = useCallback(async (manual = false) => {
    if (!isTauri()) return;
    const runId = ++assetsRunRef.current;
    setResourcesBusy(true);
    try {
      const data = await native.myAccessResources();
      if (runId !== assetsRunRef.current) return;
      setGrantRequests(data.requests ?? []);
      setGrantCatalog(data.catalog ?? { groups: [], addresses: [] });
      setGroupGrants(data.group_grants ?? []);
      setResourcesOkRun((run) => run + 1);
    } catch (cause) {
      if (runId !== assetsRunRef.current) return;
      const code = (cause as { code?: string }).code;
      if (code === "biz.unauthorized" || code === "biz.forbidden") {
        setGrantRequests([]);
        setGrantCatalog({ groups: [], addresses: [] });
        setGroupGrants([]);
        const message = formatInvokeError(cause) || trRef.current("toast.failed");
        showToastRef.current(message);
      } else if (manual) {
        showToastRef.current(formatInvokeError(cause) || trRef.current("toast.failed"));
      }
    } finally {
      if (runId === assetsRunRef.current) setResourcesBusy(false);
    }
  }, [showToastRef, trRef]);

  async function ensureApiSession(focus = false): Promise<boolean> {
    try {
      await native.acquireApiSession(focus);
      return true;
    } catch (cause) {
      showToastRef.current(formatInvokeError(cause) || trRef.current("toast.failed"));
      return false;
    }
  }

  const resourceSystems = useMemo(
    () => mapAccessResources(grantRequests, grantCatalog, groupGrants, routes, connected),
    [grantRequests, grantCatalog, groupGrants, routes, connected],
  );

  const liveRef = useRef<LivenessState | null>(null);

  useEffect(() => {
    if (!connected || resourceSystems.length === 0) {
      liveRef.current = null;
      return;
    }
    const rows = collectRows(resourceSystems);
    const previous = liveRef.current;
    const flips = livenessFlips(previous, rows);
    liveRef.current = livenessState(rows);
    if (!previous) {
      console.info(`resource liveness: baseline of ${rows.length} rows`);
      return;
    }
    const message = describeFlips(flips, trRef.current);
    if (!message) return;
    console.info(
      `resource liveness: ${flips
        .map((flip) => `${flip.address}=${flip.live ? "live" : "down"}`)
        .join(", ")} → ${windowHidden ? (notifyEnabledRef.current ? "notification" : "dropped") : "toast"}`,
    );
    if (windowHidden) return;
    showToastRef.current(message);
  }, [resourceSystems, connected, windowHidden]);

  const loadUserEmail = useCallback(async () => {
    if (!isTauri()) return;
    const info = await native.currentUser().catch(() => null);
    const email = info?.email?.trim();
    setUserEmail(email ? email : "—");
  }, []);

  const refresh = useCallback(async () => {
    if (!isTauri()) return;
    try {
      const status = await native.status();
      applyStatus(status, true);
    } catch {
      endConnectWindow();
      setConnection("error");
    }
  }, [applyStatus, endConnectWindow]);

  const burstRef = useRef<number[]>([]);
  const clearConnectBurst = useCallback(() => {
    burstRef.current.forEach((id) => window.clearTimeout(id));
    burstRef.current = [];
  }, []);
  const scheduleConnectBurst = useCallback(() => {
    clearConnectBurst();
    burstRef.current = CONNECT_BURST_MS.map((ms) =>
      window.setTimeout(() => {
        if (connectionRef.current === "connecting") void refresh();
      }, ms),
    );
  }, [clearConnectBurst, refresh]);
  useEffect(() => clearConnectBurst, [clearConnectBurst]);

  useEffect(() => {
    if (!isTauri() || connection !== "connecting") return;
    let cancelled = false;
    const tick = async () => {
      const status = await native.status().catch(() => null);
      if (!cancelled && status) applyStatus(status, true);
    };
    const id = window.setInterval(() => void tick(), CONNECT_POLL_MS);
    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
  }, [connection, applyStatus]);

  useEffect(() => {
    let alive = true;
    const announce = announceProbeRef.current;
    announceProbeRef.current = false;
    void loadProfiles();
    if (!isTauri()) {
      setMainGate("welcome");
      return () => {
        alive = false;
      };
    }
    void (async () => {
      try {
        const probe = await native.apiProbe().catch((cause) => ({
          status: "unreachable" as const,
          detail: formatInvokeError(cause) || null,
        }));
        if (!alive) return;
        if (probe.status !== "ok") {
          if (announce) {
            const message =
              probe.detail?.trim() ||
              trRef.current(probe.status === "unconfigured" ? "welcome.apiUnconfigured" : "welcome.apiDown");
            showToastRef.current(message);
          }
          setDaemonDown(false);
          setApiBlocked(probe);
          setMainGate("welcome");
          return;
        }
        setApiBlocked(null);
        const status = await native.status();
        if (!alive) return;
        setDaemonDown(false);
        if (status.state === "connected") {
          if (!(await ensureApiSession())) {
            await native.disconnect().catch(() => undefined);
            setConnection("needs-login");
            setMainGate("welcome");
            return;
          }
          applyStatus(status, true);
          setMainGate("home");
          return;
        }
        const login = await native.checkLogin();
        if (!alive) return;
        if (login.kind === "ready") {
          applyStatus(status, true);
          setMainGate("home");
          return;
        }
        applyStatus(status, false);
        setMainGate("welcome");
      } catch (cause) {
        if (!alive) return;
        if (announce) {
          showToastRef.current(formatInvokeError(cause) || trRef.current("toast.failed"));
        }
        setDaemonDown(isDaemonDown(cause));
        setMainGate("welcome");
      }
    })();
    return () => {
      alive = false;
    };
    // Bootstrap once. applyStatus identity must not retrigger Login probes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [loadProfiles, probeRun]);

  const retryBootstrap = useCallback(() => {
    announceProbeRef.current = true;
    setProbeRun((run) => run + 1);
  }, []);

  useEffect(() => {
    if (!isTauri() || !apiBlocked || mainGate !== "welcome") return;
    let inFlight = false;
    const tick = async () => {
      if (inFlight) return;
      inFlight = true;
      const probe = await native.apiProbe().catch(() => null);
      inFlight = false;
      if (probe?.status === "ok") setProbeRun((run) => run + 1);
    };
    const id = window.setInterval(() => void tick(), API_PROBE_RETRY_MS);
    return () => window.clearInterval(id);
  }, [apiBlocked, mainGate]);

  useEffect(() => {
    if (!isTauri()) return;
    let disposed = false;
    let stop: (() => void) | undefined;
    const onStatus = (status: AgentStatus) => {
      if (isDaemonLive(status.stream)) setDaemonDown(false);
      applyStatusRef.current(status, true);
    };
    void native.watchAgentStatus(onStatus).then((unlisten) => {
      if (disposed) unlisten();
      else stop = unlisten;
    });
    return () => {
      disposed = true;
      stop?.();
    };
  }, []);

  useEffect(() => {
    if (!isTauri() || mainGate !== "home" || windowHidden) return;
    void refresh();
  }, [mainGate, refresh, windowHidden]);

  useEffect(() => {
    const cameBackToVisible = windowHiddenRef.current && !windowHidden;
    windowHiddenRef.current = windowHidden;
    if (!cameBackToVisible || !isTauri()) return;
    if (mainGateRef.current !== "home" || connectionRef.current !== "connected") return;
    void loadResources();
  }, [windowHidden, loadResources]);

  useEffect(() => {
    if (!isTauri() || mainGate !== "home" || !connected) return;
    void loadResources();
  }, [connected, mainGate, loadResources]);

  useEffect(() => {
    if (!isTauri() || mainGate !== "home" || !signedIn || !connected) return;
    if (userEmail !== "—") return;
    void loadUserEmail();
  }, [mainGate, signedIn, connected, userEmail, loadUserEmail, resourcesOkRun]);

  useEffect(() => {
    if (mainGate !== "home" || connection !== "needs-login") return;
    setSessionExpired(true);
    setMainGate("welcome");
  }, [mainGate, connection]);

  useEffect(() => {
    if (!isTauri()) return;
    let disposed = false;
    let stop: (() => void) | undefined;
    void native.watchWindowVisibility((visible) => {
      if (!disposed) setWindowHidden(!visible);
    }).then((unlisten) => {
      if (disposed) unlisten();
      else stop = unlisten;
    });
    return () => {
      disposed = true;
      stop?.();
    };
  }, []);

  useEffect(() => {
    if (!isTauri() || mainGate !== "home" || !connected) return;
    if (assetRefreshMin <= 0) return;
    const id = window.setInterval(() => void loadResources(), assetRefreshMin * 60_000);
    return () => window.clearInterval(id);
  }, [mainGate, connected, assetRefreshMin, loadResources]);

  useEffect(() => {
    if (!isTauri() || !pendingApproval || mainGate !== "welcome") return;
    let cancelled = false;
    let inFlight = false;
    const tick = async () => {
      if (inFlight) return;
      inFlight = true;
      const status = await native.myMeshStatus().catch(() => null);
      inFlight = false;
      if (cancelled || !status) return;
      if (status.status === "rejected") {
        setPendingApproval(false);
        setMeshRejected(true);
        return;
      }
      if (status.status !== "approved") return;
      if (Date.now() - meshRetryAtRef.current < 60_000) return;
      meshRetryAtRef.current = Date.now();
      setPendingApproval(false);
      await startSsoRef.current();
    };
    const id = window.setInterval(() => void tick(), 5_000);
    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
  }, [pendingApproval, mainGate]);

  useEffect(() => {
    const prev = prevConnection.current;
    prevConnection.current = connection;
    if (prev === "connecting" && connection === "connected") {
      showToastRef.current(trRef.current("toast.connected"));
    }
  }, [connection]);

  useEffect(() => {
    if (mainGate === "home") void refresh();
  }, [warningLeadSecs, mainGate, refresh]);

  async function extendSession() {
    if (!isTauri()) {
      showToast(tr("toast.requiresDesktop"));
      return;
    }
    if (ssoInFlightRef.current) return;
    ssoInFlightRef.current = true;
    const runId = ++ssoRunRef.current;
    ssoActionRef.current = "extend";
    setSsoBusy(true);
    setSsoUrl("");
    try {
      const start = await native.extendSso();
      if (runId !== ssoRunRef.current) return;
      if (!start.verification_uri_complete && !start.verification_uri) {
        showToast(tr("toast.ssoNoUrl"));
        return;
      }
      setSsoUrl(ssoSignInUrl(start.verification_uri_complete, start.verification_uri));
      const status = await native.waitExtendSso(start.device_code, start.user_code);
      if (runId !== ssoRunRef.current) return;
      applyStatus(status, true);
      sessionDismissedFor.current = null;
      setSessionExpiring(false);
      showToast(tr("toast.sessionRenewed"));
    } catch (cause) {
      if (runId !== ssoRunRef.current) return;
      showToast(formatInvokeError(cause) || tr("toast.failed"));
    } finally {
      ssoInFlightRef.current = false;
      if (runId === ssoRunRef.current) setSsoBusy(false);
    }
  }

  async function dismissSessionWarning() {
    sessionDismissedFor.current = null;
    setSessionExpiring(false);
    if (isTauri()) {
      try {
        const status = await native.status();
        if (status.session_expires_at) sessionDismissedFor.current = status.session_expires_at;
        await native.dismissSessionWarning();
      } catch {
        /* best-effort */
      }
    }
  }

  async function startSso() {
    if (!isTauri()) {
      showToast(tr("toast.requiresDesktop"));
      return;
    }
    if (ssoInFlightRef.current) return;
    const meshStatus = await native.myMeshStatus().catch(() => null);
    if (ssoInFlightRef.current) return;
    if (meshStatus?.status === "pending") {
      setConnection("needs-login");
      setMainGate("welcome");
      setPendingApproval(true);
      return;
    }
    if (meshStatus?.status === "rejected") {
      setConnection("needs-login");
      setMainGate("welcome");
      setPendingApproval(false);
      setMeshRejected(true);
      return;
    }
    ssoInFlightRef.current = true;
    const runId = ++ssoRunRef.current;
    ssoActionRef.current = "login";
    setPendingApproval(false);
    setPeerConflict(false);
    setMeshRejected(false);
    setSessionExpired(false);
    setSsoBusy(true);
    setSsoUrl("");
    try {
      const result = await native.startSso();
      if (runId !== ssoRunRef.current) return;
      if (result.kind === "sso_required") {
        if (!result.verification_uri_complete && !result.verification_uri) {
          showToast(tr("toast.ssoNoUrl"));
          return;
        }
        setSsoUrl(ssoSignInUrl(result.verification_uri_complete, result.verification_uri));
        await native.waitSso(result.user_code);
        if (runId !== ssoRunRef.current) return;
      }
      if (!(await ensureApiSession(true))) {
        endConnectWindow();
        setConnection("needs-login");
        setMainGate("welcome");
        return;
      }
      beginConnectWindow();
      setConnection("connecting");
      setMainGate("home");
      scheduleConnectBurst();
      const status = await native.up();
      if (runId !== ssoRunRef.current) return;
      if (connectionRef.current === "disconnected") return;
      applyStatus(status, true);
      setPendingApproval(false);
    } catch (cause) {
      if (runId !== ssoRunRef.current) return;
      if (isPendingApprovalError(cause)) {
        endConnectWindow();
        setConnection("needs-login");
        setMainGate("welcome");
        setPendingApproval(true);
        return;
      }
      if (isPeerConflictError(cause)) {
        endConnectWindow();
        setConnection("needs-login");
        setMainGate("welcome");
        setPeerConflict(true);
        return;
      }
      endConnectWindow();
      const message = formatInvokeError(cause) || tr("toast.failed");
      showToast(message);
      if (notifyEnabledRef.current) void native.notify(tr("app.name"), message);
      setConnection((current) => (current === "connecting" ? "error" : current));
    } finally {
      ssoInFlightRef.current = false;
      if (runId === ssoRunRef.current) setSsoBusy(false);
    }
  }

  function cancelSso() {
    ssoRunRef.current += 1;
    ssoInFlightRef.current = false;
    setSsoBusy(false);
    setSsoUrl("");
  }

  function retrySso() {
    if (ssoActionRef.current === "extend") void extendSession();
    else void startSso();
  }

  async function toggleConnect() {
    if (apiBlocked) {
      retryBootstrap();
      return;
    }
    if (daemonDown) {
      retryBootstrap();
      return;
    }
    if (connection === "needs-login") {
      await startSso();
      return;
    }
    if (!isTauri()) {
      showToast(tr("toast.requiresDesktop"));
      return;
    }
    if (connecting) return;
    if (connected) {
      try {
        await native.disconnect();
      } catch {
        /* still mark down */
      }
      endConnectWindow();
      setConnection("disconnected");
      setSessionExpiring(false);
      return;
    }
    if (!(await ensureApiSession(true))) {
      endConnectWindow();
      setConnection("error");
      return;
    }
    beginConnectWindow();
    setConnection("connecting");
    scheduleConnectBurst();
    try {
      const status = await native.up();
      if (connectionRef.current === "disconnected") return;
      applyStatus(status, true);
    } catch (cause) {
      endConnectWindow();
      if (connectionRef.current === "disconnected") return;
      setConnection("error");
      const message = formatInvokeError(cause) || tr("toast.failed");
      showToast(message);
      if (notifyEnabledRef.current) void native.notify(tr("app.name"), message);
    }
  }

  async function copyText(text: string) {
    if (!text) return;
    if (!(await writeToClipboard(text))) {
      showToast(tr("toast.failed"));
      return;
    }
    setCopied(true);
    showToast(tr("toast.copied"));
    window.setTimeout(() => setCopied(false), 900);
  }

  async function openDest(address: string) {
    const href = /^https?:\/\//i.test(address) ? address : `https://${address}`;
    if (isTauri()) await native.openUrl(href);
    else window.open(href, "_blank", "noopener");
  }

  async function openSsh(row: ResourceRow) {
    if (!row.ssh || sshBusyKey) return;
    setSshBusyKey(row.key);
    try {
      const plan = await native.sshLaunch(row.ssh.requestId, addressKey(row.address));
      if (plan.status === "launched") showToast(tr("toast.sshLaunched"));
      else setSshFallback(plan);
    } catch (cause) {
      showToast(
        isNoLocalKeyError(cause)
          ? tr("resources.sshNoKey")
          : formatInvokeError(cause) || tr("toast.failed"),
      );
    } finally {
      setSshBusyKey(null);
    }
  }

  async function openSshCopy(row: ResourceRow) {
    if (!row.ssh || sshBusyKey) return;
    setSshBusyKey(row.key);
    try {
      const plan = await native.sshPrepare(row.ssh.requestId, addressKey(row.address));
      if (!(await writeToClipboard(plan.command))) {
        showToast(tr("toast.failed"));
        return;
      }
      showToast(tr("toast.copied"));
    } catch (cause) {
      showToast(
        isNoLocalKeyError(cause)
          ? tr("resources.sshNoKey")
          : formatInvokeError(cause) || tr("toast.failed"),
      );
    } finally {
      setSshBusyKey(null);
    }
  }

  function closeSshFallback() {
    setSshFallback(null);
  }

  return {
    connection,
    setConnection,
    hostname,
    userEmail,
    overlayIp,
    linkOk,
    daemonVersion,
    resourceSystems,
    resourcesBusy,
    ssoBusy,
    ssoUrl,
    pendingApproval,
    peerConflict,
    meshRejected,
    sessionExpired,
    cancelSso,
    retrySso,
    sessionExpiring,
    setSessionExpiring,
    copied,
    connected,
    connecting,
    signedIn,
    mainGate,
    daemonDown,
    apiBlocked,
    retryBootstrap,
    startSso,
    extendSession,
    dismissSessionWarning,
    toggleConnect,
    copyText,
    openDest,
    openSsh,
    openSshCopy,
    sshBusyKey,
    sshFallback,
    closeSshFallback,
    loadResources,
  };
}
