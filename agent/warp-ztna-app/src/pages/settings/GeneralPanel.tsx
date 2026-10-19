import { useEffect, useState } from "react";
import type { Lang } from "../../i18n";
import type { Prefs, ThemePref } from "../../types/ui";
import type { ProfileRow } from "../../types/agent";
import type { ServiceConfigState } from "../../hooks/useServiceConfig";

export function GeneralPanel({
  lang,
  themePref,
  prefs,
  active,
  serviceConfig,
  tr,
  onLang,
  onTheme,
  onPref,
  onSessionLead,
  onAssetRefresh,
  onManageProfiles,
}: {
  lang: Lang;
  themePref: ThemePref;
  prefs: Prefs;
  active?: ProfileRow;
  serviceConfig: ServiceConfigState;
  tr: (key: string) => string;
  onLang: (lang: Lang) => void;
  onTheme: (theme: ThemePref) => void;
  onPref: (key: "notify" | "startup" | "launch", value: boolean, storageKey: string) => void;
  onSessionLead: (minutes: number) => void;
  onAssetRefresh: (minutes: number) => void;
  onManageProfiles: () => void;
}) {
  const [leadDraft, setLeadDraft] = useState(String(prefs.sessionWarningLeadMin));
  useEffect(() => {
    setLeadDraft(String(prefs.sessionWarningLeadMin));
  }, [prefs.sessionWarningLeadMin]);
  const [refreshDraft, setRefreshDraft] = useState(String(prefs.assetRefreshMin));
  useEffect(() => {
    setRefreshDraft(String(prefs.assetRefreshMin));
  }, [prefs.assetRefreshMin]);

  return (
    <div className="s-panel active">
      <header className="s-intro">
        <h3>{tr("settings.general")}</h3>
        <p className="hint">{tr("settings.generalSub")}</p>
      </header>
      <div className="s-card">
        <div className="row">
          <div>
            <span>{tr("settings.language")}</span>
            <p className="row-sub">{tr("settings.languageSub")}</p>
          </div>
          <div className="seg">
            <button type="button" className={`seg-btn${lang === "en" ? " active" : ""}`} onClick={() => onLang("en")}>
              English
            </button>
            <button type="button" className={`seg-btn${lang === "zh" ? " active" : ""}`} onClick={() => onLang("zh")}>
              简体中文
            </button>
          </div>
        </div>
      </div>
      <div className="s-card">
        <div className="row">
          <div>
            <span>{tr("settings.currentServer")}</span>
            <code className="server-url">{active?.url || "—"}</code>
          </div>
          <button type="button" className="btn btn-ghost btn-sm" onClick={onManageProfiles}>
            {tr("settings.manageProfiles")}
          </button>
        </div>
      </div>
      <div className="s-card">
        <div className="row">
          <div>
            <span>{tr("serviceConfig.title")}</span>
            <p className="row-sub">{tr("serviceConfig.hint")}</p>
          </div>
          <button
            type="button"
            className="btn btn-primary btn-sm"
            onClick={() => void serviceConfig.save()}
            disabled={serviceConfig.saving}
          >
            {tr("action.save")}
          </button>
        </div>
        <label className="row">
          <span>{tr("serviceConfig.api")}</span>
          <span className="svc-field">
            <input
              value={serviceConfig.apiUrl}
              onChange={(e) => serviceConfig.setApiUrl(e.target.value)}
              placeholder={serviceConfig.effectiveApiUrl}
              aria-label={tr("serviceConfig.api")}
            />
          </span>
        </label>
        {serviceConfig.needRestart && (
          <div className="row">
            <p className="row-sub">{tr("serviceConfig.saved")}</p>
            <button type="button" className="btn btn-ghost btn-sm" onClick={serviceConfig.restart}>
              {tr("serviceConfig.restart")}
            </button>
          </div>
        )}
      </div>
      <div className="s-card">
        <div className="row">
          <span>{tr("settings.appearance")}</span>
          <div className="seg">
            {(["dark", "light", "system"] as const).map((th) => (
              <button
                key={th}
                type="button"
                className={`seg-btn${themePref === th ? " active" : ""}`}
                onClick={() => onTheme(th)}
              >
                {th === "dark" ? (lang === "en" ? "Dark" : "深色") : th === "light" ? (lang === "en" ? "Light" : "浅色") : lang === "en" ? "System" : "系统"}
              </button>
            ))}
          </div>
        </div>
        <label className="row">
          <span>{tr("settings.notifications")}</span>
          <span className="switch">
            <input
              type="checkbox"
              checked={prefs.notify}
              onChange={(e) => onPref("notify", e.target.checked, "warp-ztna-opt-notify")}
            />
            <span className="switch-ui" />
          </span>
        </label>
        <label className="row">
          <span>{tr("settings.connectOnStartup")}</span>
          <span className="switch">
            <input
              type="checkbox"
              checked={prefs.startup}
              onChange={(e) => onPref("startup", e.target.checked, "warp-ztna-opt-startup")}
            />
            <span className="switch-ui" />
          </span>
        </label>
        <label className="row">
          <div>
            <span>{tr("settings.sessionLead")}</span>
            <p className="row-sub">{tr("settings.sessionLeadSub")}</p>
          </div>
          <span className="lead-field">
            <input
              type="number"
              min={1}
              max={59}
              value={leadDraft}
              onChange={(e) => setLeadDraft(e.target.value)}
              onBlur={() => onSessionLead(Number(leadDraft))}
              aria-label={tr("settings.sessionLead")}
            />
            <span className="row-sub">{tr("settings.sessionLeadUnit")}</span>
          </span>
        </label>
        <label className="row">
          <div>
            <span>{tr("settings.assetRefresh")}</span>
            <p className="row-sub">{tr("settings.assetRefreshSub")}</p>
          </div>
          <span className="lead-field">
            <input
              type="number"
              min={0}
              max={60}
              value={refreshDraft}
              onChange={(e) => setRefreshDraft(e.target.value)}
              onBlur={() => onAssetRefresh(Number(refreshDraft))}
              aria-label={tr("settings.assetRefresh")}
            />
            <span className="row-sub">{tr("settings.assetRefreshUnit")}</span>
          </span>
        </label>
        <label className="row">
          <span>{tr("settings.launchAtLogin")}</span>
          <span className="switch">
            <input
              type="checkbox"
              checked={prefs.launch}
              onChange={(e) => onPref("launch", e.target.checked, "warp-ztna-opt-launch")}
            />
            <span className="switch-ui" />
          </span>
        </label>
      </div>
    </div>
  );
}
