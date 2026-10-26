import {
  IconArrow,
  IconCheck,
  IconExtLink,
  IconRefresh,
  IconWarn,
} from "../../components/Icons";
import { clientUpdateDetail, clientUpdateView, type ClientUpdate } from "../../lib/updateCheck";

const NETBIRD_OWNER = "NetBird GmbH";
const NETBIRD_COPYRIGHT = "© 2022 NetBird GmbH & AUTHORS";

function updateStateClassName(update: ClientUpdate | null, busy: boolean): string {
  if (busy) return "about-update-state is-checking";
  if (update?.status === "latest") return "about-update-state is-latest";
  if (update?.status === "unknown" || update?.status === "unreachable")
    return "about-update-state is-unknown";
  return "about-update-state";
}

function updateStateIcon(update: ClientUpdate | null, busy: boolean) {
  if (busy) return <IconRefresh />;
  if (update?.status === "latest") return <IconCheck />;
  if (update?.status === "unknown" || update?.status === "unreachable") return <IconWarn />;
  return null;
}

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
          <div className="about-update" role="status" aria-live="polite">
            {updateView.canDownload ? (
              <>
                <button type="button" className="about-update-row" onClick={onDownloadUpdate}>
                  <span className="about-update-name">
                    {tr(updateView.key, updateView.vars)}
                  </span>
                  <span className="about-update-cta">
                    {tr("about.updateDownload")}
                    <IconArrow />
                  </span>
                </button>
                <p className="about-update-hint">{tr("about.updateHint")}</p>
              </>
            ) : (
              <>
                <p className={updateStateClassName(update, updateBusy)}>
                  {updateStateIcon(update, updateBusy)}
                  <span>{tr(updateView.key, updateView.vars)}</span>
                </p>
                {updateDetail && <p className="about-update-detail">{updateDetail}</p>}
              </>
            )}
          </div>
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
