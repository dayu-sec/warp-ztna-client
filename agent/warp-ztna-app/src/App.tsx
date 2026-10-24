import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { collectRows, type ResourceRow } from "./api/access";
import { isTauri, native } from "./api/native";
import { Titlebar } from "./components/Titlebar";
import { ConfirmSwitch } from "./components/overlays/ConfirmSwitch";
import { ProfileDialog } from "./components/overlays/ProfileDialog";
import { SessionOverlay } from "./components/overlays/SessionOverlay";
import { SsoOverlay } from "./components/overlays/SsoOverlay";
import {
  SshFallbackDialog,
  sshFallbackCommands,
  sshFallbackReason,
} from "./components/overlays/SshFallbackDialog";
import { Toast } from "./components/overlays/Toast";
import { useAgent } from "./hooks/useAgent";
import { usePrefs } from "./hooks/usePrefs";
import { useProfiles } from "./hooks/useProfiles";
import { useServiceConfig } from "./hooks/useServiceConfig";
import { detectPlatform, logoSrc } from "./lib/platform";
import { HomePage } from "./pages/HomePage";
import { WelcomePage } from "./pages/WelcomePage";
import { SettingsPage } from "./pages/settings/SettingsPage";
import type { ProfileRow } from "./types/agent";
import type { SettingsTab, Surface } from "./types/ui";

const TRAY_RESOURCE_LIMIT = 15;

export function App() {
  const [surface, setSurface] = useState<Surface>("main");
  const [settingsTab, setSettingsTab] = useState<SettingsTab>("general");
  const [toast, setToast] = useState<string | null>(null);
  const toastTimerRef = useRef(0);
  const [maximized, setMaximized] = useState(false);
  const [appVersion, setAppVersion] = useState("");
  const didRevealWindowRef = useRef(false);
  const stageRef = useRef<HTMLDivElement>(null);

  const showToast = (msg: string) => {
    setToast(msg);
    window.clearTimeout(toastTimerRef.current);
    toastTimerRef.current = window.setTimeout(() => setToast(null), 2400);
  };

  const {
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
  } = usePrefs();
  const profiles = useProfiles(tr, showToast);
  const serviceConfig = useServiceConfig(tr, showToast);
  const agent = useAgent(
    tr,
    showToast,
    profiles.loadProfiles,
    prefs.sessionWarningLeadMin,
    prefs.assetRefreshMin,
    prefs.notify,
  );
  const platform = detectPlatform();
  const logo = logoSrc(agent.connected);
  const agentRef = useRef(agent);
  const profilesRef = useRef(profiles);
  agentRef.current = agent;
  profilesRef.current = profiles;

  const toggleConnect = useCallback(() => void agentRef.current.toggleConnect(), []);
  const copyText = useCallback((text: string) => void agentRef.current.copyText(text), []);
  const openDest = useCallback((address: string) => void agentRef.current.openDest(address), []);
  const openSsh = useCallback((row: ResourceRow) => void agentRef.current.openSsh(row), []);
  const openSshCopy = useCallback((row: ResourceRow) => void agentRef.current.openSshCopy(row), []);
  const requestAccess = useCallback(() => {
    void (isTauri() ? native.openPortal("request") : undefined);
  }, []);
  const refreshAssets = useCallback(() => void agentRef.current.loadResources(true), []);

  useEffect(() => {
    let disposed = false;
    let stop: (() => void) | undefined;
    void native.watchMaximized((value) => {
      if (!disposed) setMaximized(value);
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
    stageRef.current?.focus();
  }, []);

  useEffect(() => {
    void native.appVersion().then(setAppVersion).catch(() => undefined);
  }, []);

  useEffect(() => {
    let disposed = false;
    let stop: (() => void) | undefined;
    void listen("tray-open-settings", () => {
      if (!disposed) {
        setSurface("settings");
        setSettingsTab("general");
      }
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
    if (!isTauri()) return;
    let disposed = false;
    void native
      .frontendReady()
      .then((action) => {
        if (!disposed && action === "settings") {
          setSurface("settings");
          setSettingsTab("general");
        }
      })
      .catch(() => undefined);
    return () => {
      disposed = true;
    };
  }, []);

  useEffect(() => {
    if (isTauri()) void native.setTrayIconTheme(systemTheme);
  }, [systemTheme]);

  const trayMenu = useMemo(() => {
    const rows = collectRows(agent.resourceSystems);
    const resources = rows.slice(0, TRAY_RESOURCE_LIMIT).map((row) => ({
      label: row.address,
      status: tr(row.effective ? "resources.live" : "resources.pending"),
    }));
    const more = rows.length > TRAY_RESOURCE_LIMIT;
    const body = resources.map((item) => `${item.label}|${item.status}`).join("|");
    return {
      resources,
      more,
      signature: `${lang}|${agent.connected}|${agent.apiBlocked !== null}|${more}|${body}`,
    };
  }, [agent.resourceSystems, agent.connected, agent.apiBlocked, lang, tr]);

  const trayPushedRef = useRef("");

  useEffect(() => {
    if (!isTauri()) return;
    if (trayPushedRef.current === trayMenu.signature) return;
    trayPushedRef.current = trayMenu.signature;
    void native.setTrayMenu(
      {
        open: tr("tray.open"),
        request: tr("action.requestAccess"),
        resources: tr("tray.resources"),
        connect: tr("action.connect"),
        disconnect: tr("action.disconnect"),
        settings: tr("nav.settings"),
        quit: tr("tray.quit"),
        more: tr("tray.resourcesMore"),
      },
      agent.connected,
      trayMenu.resources,
      trayMenu.more,
      agent.apiBlocked !== null,
    );
  }, [trayMenu, agent.connected, tr]);

  useEffect(() => {
    if (agent.mainGate === "pending") return;
    const slim = surface === "main" && agent.mainGate === "welcome";
    const next = slim
      ? { width: 620, height: 659 }
      : { width: 912, height: 659 };
    if (!isTauri()) return;
    void native.setWindowSize(next.width, next.height).finally(() => {
      if (!didRevealWindowRef.current) {
        didRevealWindowRef.current = true;
        void native.showWindow();
      }
    });
  }, [surface, agent.mainGate]);

  useEffect(() => {
    const html = document.documentElement;
    html.setAttribute("data-theme", theme);
    html.setAttribute("data-platform", platform);
    html.setAttribute("data-native", "1");
    html.setAttribute("data-connection", agent.connection === "needs-login" ? "needs-login" : agent.connection);
    html.toggleAttribute("data-expiring", agent.connected && agent.sessionExpiring);
    html.toggleAttribute("data-maximized", maximized);
  }, [theme, platform, agent.connection, agent.connected, agent.sessionExpiring, maximized]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const current = agentRef.current;
      const profileState = profilesRef.current;
      const inField = /^(INPUT|TEXTAREA|SELECT)$/.test((e.target as HTMLElement).tagName);
      const meta = e.metaKey || e.ctrlKey;
      if (e.key === "Escape") {
        current.cancelSso();
        profileState.setProfileModal(null);
        profileState.setConfirmSwitch(null);
        if (!current.sessionExpiring) current.setSessionExpiring(false);
      }
      if (e.key === "/" && !inField) {
        e.preventDefault();
        document.getElementById("search-res")?.focus();
      }
      if (meta && e.key === "s") {
        e.preventDefault();
        setSurface("settings");
      }
      if (meta && e.key === "m") {
        e.preventDefault();
        setSurface("main");
      }
      if (meta && e.key === "Enter") {
        e.preventDefault();
        void current.toggleConnect();
      }
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, []);

  function requestSwitch(p: ProfileRow) {
    if (agent.connected || agent.connecting) {
      profiles.setConfirmSwitch(p);
      return;
    }
    void profiles.doSwitch(p).then((ok) => {
      if (ok) agent.setConnection("disconnected");
    });
  }

  const statusKey =
    agent.connected && agent.sessionExpiring
      ? "status.sessionExpiring"
      : agent.connection === "needs-login"
        ? "status.needsLogin"
        : `status.${agent.connection}`;
  const heroSub =
    agent.connection === "connected"
      ? tr(agent.linkOk ? "hero.sub.connected" : "hero.sub.linkDegraded")
      : agent.connection === "connecting"
        ? tr("hero.sub.connecting")
        : agent.connection === "reconnecting"
          ? tr("hero.sub.reconnecting")
          : agent.connection === "error"
            ? tr("hero.sub.error")
            : tr("hero.sub.disconnected");
  const orbCaption = agent.connected
    ? tr("action.clickDisconnect")
    : agent.connection === "error"
      ? tr("action.retry")
      : agent.connecting
        ? tr("status.connecting")
        : tr("action.clickConnect");
  const welcomeNotice = agent.peerConflict
    ? tr("welcome.peerConflict")
    : agent.meshRejected
      ? tr("welcome.meshRejected")
      : agent.pendingApproval
        ? tr("welcome.pendingApproval")
        : agent.sessionExpired
          ? tr("welcome.sessionExpired")
          : undefined;
  const welcomeProfileName = profiles.profiles.length > 1 ? profiles.active?.name : undefined;

  return (
    <div className="stage" ref={stageRef} tabIndex={-1}>
      <div
        className="app-shell"
        data-theme={theme}
        data-connection={agent.connection === "needs-login" ? "needs-login" : agent.connection}
        data-platform={platform}
        data-native="1"
        {...(agent.sessionExpiring && agent.connected ? { "data-expiring": "true" } : {})}
      >
        <div className="ambient-glow" aria-hidden="true" />

        <Titlebar
          lang={lang}
          theme={theme}
          surface={surface}
          platform={platform}
          maximized={maximized}
          onBack={() => setSurface("main")}
          onOpenSettings={() => {
            setSurface("settings");
            setSettingsTab("general");
          }}
          onToggleTheme={toggleTheme}
        />

        {surface === "main" && (
          <div id="surface-main">
            {agent.mainGate === "pending" ? null : agent.mainGate === "welcome" ? (
              <WelcomePage
                logo={logo}
                title="Warp-ZTNA"
                subtitle={tr("welcome.sub")}
                actionLabel={
                  agent.apiBlocked || agent.daemonDown ? tr("action.retry") : tr("action.signInSso")
                }
                hint={
                  agent.apiBlocked
                    ? tr(
                        agent.apiBlocked.status === "unconfigured"
                          ? "welcome.apiUnconfigured"
                          : "welcome.apiDown",
                      )
                    : agent.daemonDown
                      ? tr("welcome.daemonDown")
                      : tr("welcome.mgmtHint")
                }
                notice={agent.apiBlocked ? agent.apiBlocked.detail?.trim() || undefined : welcomeNotice}
                profileLabel={tr("welcome.usingProfile")}
                profileName={welcomeProfileName}
                onAction={() =>
                  agent.apiBlocked || agent.daemonDown
                    ? agent.retryBootstrap()
                    : void agent.startSso()
                }
              />
            ) : (
              <HomePage
                connection={agent.connection}
                connected={agent.connected}
                connecting={agent.connecting}
                signedIn={agent.signedIn}
                logo={logo}
                statusLabel={tr(statusKey)}
                heroSub={heroSub}
                orbCaption={orbCaption}
                overlayIp={agent.overlayIp}
                hostname={agent.hostname}
                userEmail={agent.userEmail}
                linkOk={agent.linkOk}
                copied={agent.copied}
                resourceSystems={agent.resourceSystems}
                resourcesBusy={agent.resourcesBusy}
                sshBusyKey={agent.sshBusyKey}
                tr={tr}
                onToggleConnect={toggleConnect}
                onCopy={copyText}
                onOpenDest={openDest}
                onSsh={openSsh}
                onSshCopy={openSshCopy}
                onRequestAccess={requestAccess}
                onRefreshAssets={refreshAssets}
              />
            )}
          </div>
        )}

        {surface === "settings" && (
          <SettingsPage
            tab={settingsTab}
            lang={lang}
            themePref={themePref}
            prefs={prefs}
            profiles={profiles.profiles}
            active={profiles.active}
            serviceConfig={serviceConfig}
            logo={logo}
            appName={tr("app.name")}
            appVersion={appVersion}
            daemonVersion={agent.daemonVersion}
            tr={tr}
            onTab={setSettingsTab}
            onLang={setLang}
            onTheme={setThemePref}
            onPref={updatePref}
            onSessionLead={setSessionWarningLeadMin}
            onAssetRefresh={setAssetRefreshMin}
            onNewProfile={() => {
              profiles.setPfName("");
              profiles.setPfUrl("");
              profiles.setProfileModal({ id: null });
            }}
            onUseProfile={requestSwitch}
            onEditProfile={(p) => {
              profiles.setPfName(p.name);
              profiles.setPfUrl(p.url);
              profiles.setProfileModal({ id: p.id || p.name });
            }}
            onDeleteProfile={(p) => void profiles.deleteProfile(p)}
            onPortal={() => void native.openPortal()}
            onHelp={() => void native.openHelp()}
            onCheckUpdate={() => showToast(tr("toast.updateLatest"))}
          />
        )}

        {agent.ssoBusy && (
          <SsoOverlay
            message={tr("welcome.ssoProgress")}
            url={agent.ssoUrl}
            copyLabel={tr("welcome.ssoCopyLink")}
            cancelLabel={tr("action.cancel")}
            retryLabel={tr("welcome.ssoRetry")}
            onCopy={(text) => void agent.copyText(text)}
            onCancel={agent.cancelSso}
            onRetry={agent.retrySso}
          />
        )}

        {agent.connected && agent.sessionExpiring && !agent.ssoBusy && (
          <SessionOverlay
            title={tr("session.title")}
            body={tr("session.body")}
            renewLabel={tr("session.renew")}
            laterLabel={tr("session.later")}
            disconnectLabel={tr("action.disconnect")}
            onRenew={() => void agent.extendSession()}
            onLater={() => void agent.dismissSessionWarning()}
            onDisconnect={() => {
              agent.setSessionExpiring(false);
              void agent.toggleConnect();
            }}
          />
        )}

        {profiles.profileModal && (
          <ProfileDialog
            title={profiles.profileModal.id ? tr("profiles.edit") : tr("profiles.create")}
            hint={tr("profiles.noCloud")}
            name={profiles.pfName}
            url={profiles.pfUrl}
            nameLabel={tr("profiles.name")}
            urlLabel={tr("profiles.url")}
            namePh={tr("profiles.namePh")}
            urlPh={tr("profiles.urlPh")}
            cancelLabel={tr("action.cancel")}
            saveLabel={tr("profiles.save")}
            onName={profiles.setPfName}
            onUrl={profiles.setPfUrl}
            onCancel={() => profiles.setProfileModal(null)}
            onSave={() => void profiles.saveProfile()}
          />
        )}

        {profiles.confirmSwitch && (
          <ConfirmSwitch
            title={tr("profiles.switchConfirm", { name: profiles.confirmSwitch.name })}
            okLabel={tr("profiles.switchOk")}
            cancelLabel={tr("action.cancel")}
            onOk={() => {
              const p = profiles.confirmSwitch;
              profiles.setConfirmSwitch(null);
              if (p) {
                void profiles.doSwitch(p).then((ok) => {
                  if (ok) agent.setConnection("disconnected");
                });
              }
            }}
            onCancel={() => profiles.setConfirmSwitch(null)}
          />
        )}

        {agent.sshFallback && (
          <SshFallbackDialog
            title={tr("ssh.fallbackTitle")}
            body={tr("ssh.fallbackBody")}
            reason={sshFallbackReason(agent.sshFallback, tr)}
            commands={sshFallbackCommands(agent.sshFallback)}
            copyLabel={tr("ssh.copyCommand")}
            closeLabel={tr("ssh.close")}
            onCopy={(text) => void agent.copyText(text)}
            onClose={agent.closeSshFallback}
          />
        )}

        {toast && <Toast message={toast} />}
        {profiles.error && surface === "settings" && <div className="sr-live">{profiles.error}</div>}
      </div>
    </div>
  );
}
