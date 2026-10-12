import { IconExtLink, IconRefresh } from "../../components/Icons";

const NETBIRD_OWNER = "NetBird GmbH";
const NETBIRD_COPYRIGHT = "© 2022 NetBird GmbH & AUTHORS";

export function AboutPanel({
  logo,
  name,
  appVersion,
  daemonVersion,
  tr,
  onPortal,
  onHelp,
  onCheckUpdate,
}: {
  logo: string;
  name: string;
  appVersion: string;
  daemonVersion: string;
  tr: (key: string, vars?: Record<string, string>) => string;
  onPortal: () => void;
  onHelp: () => void;
  onCheckUpdate: () => void;
}) {
  const usageParts = tr("about.netbirdUsage").split("{owner}");

  return (
    <div className="s-panel active">
      <div className="about-card">
        <div className="about-logo-wrap">
          <span className="logo-halo soft" aria-hidden="true" />
          <img src={logo} width={80} height={80} alt="" />
        </div>
        <h3>{name}</h3>
        <p className="about-product">Desktop</p>
        <div className="about-chip">
          <span>{appVersion}</span>
          <span className="dot" />
          <span>NetBird {daemonVersion}</span>
        </div>
        <div className="about-links">
          <button type="button" onClick={onPortal}>
            <span>{tr("about.portal")}</span>
            <IconExtLink />
          </button>
          <button type="button" onClick={onHelp}>
            <span>{tr("about.help")}</span>
            <IconExtLink />
          </button>
          <button type="button" onClick={onCheckUpdate}>
            <span>{tr("about.checkUpdate")}</span>
            <IconRefresh />
          </button>
        </div>
        <p className="about-attribution">
          {usageParts[0]}
          <span className="about-owner" tabIndex={0}>
            {NETBIRD_OWNER}
            <span className="hover-note">
              {tr("about.netbirdLicense", { copyright: NETBIRD_COPYRIGHT })}
            </span>
          </span>
          {usageParts[1]}
        </p>
      </div>
    </div>
  );
}
