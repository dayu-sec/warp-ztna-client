import { IconExtLink, IconRefresh } from "../../components/Icons";
import { clientUpdateDetail, clientUpdateView, type ClientUpdate } from "../../lib/updateCheck";

const NETBIRD_OWNER = "NetBird GmbH";
const NETBIRD_COPYRIGHT = "© 2022 NetBird GmbH & AUTHORS";

export function AboutPanel({
  logo,
  name,
  appVersion,
  daemonVersion,
  update,
  updateBusy,
  tr,
  onPortal,
  onHelp,
  onCheckUpdate,
  onDownloadUpdate,
}: {
  logo: string;
  name: string;
  appVersion: string;
  daemonVersion: string;
  update: ClientUpdate | null;
  updateBusy: boolean;
  tr: (key: string, vars?: Record<string, string>) => string;
  onPortal: () => void;
  onHelp: () => void;
  onCheckUpdate: () => void;
  onDownloadUpdate: () => void;
}) {
  const usageParts = tr("about.netbirdUsage").split("{owner}");
  const updateView = clientUpdateView(update, updateBusy);
  const updateDetail = clientUpdateDetail(update);

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
          <button type="button" onClick={onCheckUpdate} disabled={updateBusy}>
            <span>{tr("about.checkUpdate")}</span>
            <IconRefresh />
          </button>
        </div>
        <div className="about-update" role="status" aria-live="polite">
          <p className="about-update-text">{tr(updateView.key, updateView.vars)}</p>
          {updateDetail && <p className="about-update-detail">{updateDetail}</p>}
          {updateView.canDownload && (
            <button
              type="button"
              className="btn btn-ghost btn-sm"
              onClick={onDownloadUpdate}
            >
              {tr("about.updateDownload")}
            </button>
          )}
          {updateView.canDownload && <p className="about-update-hint">{tr("about.updateHint")}</p>}
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
