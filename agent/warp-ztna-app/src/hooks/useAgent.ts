import { useCallback, useEffect, useMemo, useRef, useState } from "react";
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

const API_PROBE_RETRY_MS = 5_000;

export function useAgent(
  tr: (key: string, vars?: Record<string, string>) => string,
  showToast: (msg: string) => void,
  loadProfiles: () => Promise<void>,
  warningLeadMin = 15,
  assetRefreshMin = 5,
  connectOnStartup = false,
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
  const prevExpiring = useRef(false);
  const notifyEnabledRef = useRef(notifyEnabled);
  notifyEnabledRef.current = notifyEnabled;
  const assetsRunRef = useRef(0);

  const connected = connection === "connected";
  const connecting = connection === "connecting";
  const signedIn = connection !== "needs-login";
  const connectionRef = useRef(connection);
  connectionRef.current = connection;
  const showToastRef = useRef(showToast);
  showToastRef.current = showToast;
  const trRef = useRef(tr);
  trRef.current = tr;
  const startSsoRef = useRef(startSso);
  startSsoRef.current = startSso;

  const applyStatus = useCallback((status: AgentStatus, signed: boolean, leadSecs = warningLeadSecs) => {
    setConnection((prev) => {
      const next = nextConnection(prev, status, signed);
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
  }, [warningLeadSecs]);
  const applyStatusRef = useRef(applyStatus);
  applyStatusRef.current = applyStatus;

  const loadResources = useCallback(async () => {
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
      }
    } finally {
      if (runId === assetsRunRef.current) setResourcesBusy(false);
    }
  }, [showToastRef, trRef]);

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
    if (windowHidden) {
      if (notifyEnabledRef.current) void native.notify(trRef.current("app.name"), message);
      return;
    }
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
      setConnection("error");
    }
  }, [applyStatus]);

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
          applyStatus(status, true);
          setMainGate("home");
          await loadResources();
          return;
        }
        const login = await native.checkLogin();
        if (!alive) return;
        if (login.kind === "ready") {
          const autoUp = connectOnStartup;
          if (autoUp) {
            applyStatus({ ...status, state: "connecting" }, true);
            setMainGate("home");
            if (status.state !== "connecting") {
              try {
                const next = await native.up();
                if (!alive || connectionRef.current === "disconnected") return;
                applyStatus(next, true);
                await loadResources();
              } catch {
                if (alive && connectionRef.current !== "disconnected") setConnection("error");
              }
            }
          } else {
            applyStatus(status, true);
            setMainGate("home");
          }
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
    if (!isTauri() || mainGate !== "home" || !signedIn) return;
    if (userEmail !== "—") return;
    void loadUserEmail();
  }, [mainGate, signedIn, userEmail, loadUserEmail, resourcesOkRun]);

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
    if (!isTauri() || mainGate !== "home" || !connected || windowHidden) return;
    if (assetRefreshMin <= 0) return;
    const id = window.setInterval(() => void loadResources(), assetRefreshMin * 60_000);
    return () => window.clearInterval(id);
  }, [mainGate, connected, windowHidden, assetRefreshMin, loadResources]);

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
      if (notifyEnabledRef.current) {
        void native.notify(trRef.current("app.name"), trRef.current("toast.connected"));
      }
    }
  }, [connection]);

  useEffect(() => {
    const prev = prevExpiring.current;
    prevExpiring.current = sessionExpiring;
    if (!prev && sessionExpiring && notifyEnabledRef.current) {
      void native.notify(trRef.current("session.title"), trRef.current("session.body"));
    }
  }, [sessionExpiring]);

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
      setConnection("connecting");
      setMainGate("home");
      scheduleConnectBurst();
      const status = await native.up();
      if (runId !== ssoRunRef.current) return;
      if (connectionRef.current === "disconnected") return;
      applyStatus(status, true);
      setPendingApproval(false);
      await loadResources();
    } catch (cause) {
      if (runId !== ssoRunRef.current) return;
      if (isPendingApprovalError(cause)) {
        setConnection("needs-login");
        setMainGate("welcome");
        setPendingApproval(true);
        return;
      }
      if (isPeerConflictError(cause)) {
        setConnection("needs-login");
        setMainGate("welcome");
        setPeerConflict(true);
        return;
      }
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
      setConnection("disconnected");
      setSessionExpiring(false);
      return;
    }
    setConnection("connecting");
    scheduleConnectBurst();
    try {
      const status = await native.up();
      if (connectionRef.current === "disconnected") return;
      applyStatus(status, true);
      await loadResources();
    } catch (cause) {
      if (connectionRef.current === "disconnected") return;
      setConnection("error");
      const message = formatInvokeError(cause) || tr("toast.failed");
      showToast(message);
      if (notifyEnabledRef.current) void native.notify(tr("app.name"), message);
    }
  }

  async function copyText(text: string) {
    if (!text) return;
    await navigator.clipboard?.writeText(text).catch(() => undefined);
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
      else if (plan.status === "needs_config") showToast(tr("toast.sshNeedsConfig"));
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
    sshBusyKey,
    sshFallback,
    closeSshFallback,
    loadResources,
  };
}
