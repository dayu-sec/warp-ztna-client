import { SETTINGS_ICONS } from "../../components/Icons";
import type { Lang } from "../../i18n";
import type { Prefs, SettingsTab, ThemePref } from "../../types/ui";
import type { ProfileRow } from "../../types/agent";
import type { ServiceConfigState } from "../../hooks/useServiceConfig";
import { AboutPanel } from "./AboutPanel";
import { GeneralPanel } from "./GeneralPanel";
import { ProfilesPanel } from "./ProfilesPanel";

export function SettingsPage({
  tab,
  lang,
  themePref,
  prefs,
  profiles,
  active,
  serviceConfig,
  logo,
  appName,
  appVersion,
  daemonVersion,
  tr,
  onTab,
  onLang,
  onTheme,
  onPref,
  onSessionLead,
  onAssetRefresh,
  onNewProfile,
  onUseProfile,
  onEditProfile,
  onDeleteProfile,
  onPortal,
  onHelp,
  onCheckUpdate,
}: {
  tab: SettingsTab;
  lang: Lang;
  themePref: ThemePref;
  prefs: Prefs;
  profiles: ProfileRow[];
  active?: ProfileRow;
  serviceConfig: ServiceConfigState;
  logo: string;
  appName: string;
  appVersion: string;
  daemonVersion: string;
  tr: (key: string) => string;
  onTab: (tab: SettingsTab) => void;
  onLang: (lang: Lang) => void;
  onTheme: (theme: ThemePref) => void;
  onPref: (key: "notify" | "startup" | "launch", value: boolean, storageKey: string) => void;
  onSessionLead: (minutes: number) => void;
  onAssetRefresh: (minutes: number) => void;
  onNewProfile: () => void;
  onUseProfile: (p: ProfileRow) => void;
  onEditProfile: (p: ProfileRow) => void;
  onDeleteProfile: (p: ProfileRow) => void;
  onPortal: () => void;
  onHelp: () => void;
  onCheckUpdate: () => void;
}) {
  return (
    <div id="surface-settings">
      <div className="settings-body">
        <nav className="settings-nav">
          {(["general", "profiles", "about"] as const).map((id) => (
            <button
              key={id}
              type="button"
              className={`s-nav${tab === id ? " active" : ""}`}
              onClick={() => onTab(id)}
            >
              <span className="s-nav-ico" aria-hidden="true">
                {SETTINGS_ICONS[id]}
              </span>
              <span>{tr(`settings.${id}`)}</span>
            </button>
          ))}
        </nav>
        <div className="settings-content">
          {tab === "general" && (
            <GeneralPanel
              lang={lang}
              themePref={themePref}
              prefs={prefs}
              active={active}
              serviceConfig={serviceConfig}
              tr={tr}
              onLang={onLang}
              onTheme={onTheme}
              onPref={onPref}
              onSessionLead={onSessionLead}
              onAssetRefresh={onAssetRefresh}
              onManageProfiles={() => onTab("profiles")}
            />
          )}
          {tab === "profiles" && (
            <ProfilesPanel
              profiles={profiles}
              tr={tr}
              onNew={onNewProfile}
              onUse={onUseProfile}
              onEdit={onEditProfile}
              onDelete={onDeleteProfile}
            />
          )}
          {tab === "about" && (
            <AboutPanel
              logo={logo}
              name={appName}
              appVersion={appVersion}
              daemonVersion={daemonVersion}
              tr={tr}
              onPortal={onPortal}
              onHelp={onHelp}
              onCheckUpdate={onCheckUpdate}
            />
          )}
        </div>
      </div>
    </div>
  );
}
