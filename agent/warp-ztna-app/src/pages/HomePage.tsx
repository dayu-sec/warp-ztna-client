import { memo, useMemo, useState, type ReactNode } from "react";
import {
  UNGROUPED_SYSTEM,
  collectRows,
  isLaunchable,
  type ResourceRow,
  type ResourceSystem,
} from "../api/access";
import {
  IconArrow,
  IconCheck,
  IconChevron,
  IconClear,
  IconCommandCopy,
  IconCopy,
  IconEmptyLock,
  IconFolderTree,
  IconHost,
  IconIp,
  IconKindDomain,
  IconKindSubnet,
  IconLaunch,
  IconList,
  IconPending,
  IconRefresh,
  IconSearch,
  IconSourceGroup,
  IconSourceTicket,
  IconStar,
  IconTerminal,
  IconUser,
} from "../components/Icons";
import type { UiConn } from "../types/ui";

type KindKey =
  | "resources.kindHost"
  | "resources.kindSubnet"
  | "resources.kindDomain"
  | "resources.networkWildcardDomain";

function kindKeyOf(row: ResourceRow): KindKey {
  if (row.kind === "host") return "resources.kindHost";
  if (row.kind === "subnet") return "resources.kindSubnet";
  return row.address.includes("*") ? "resources.networkWildcardDomain" : "resources.kindDomain";
}

function kindIconOf(row: ResourceRow) {
  if (row.kind === "host") return <IconHost />;
  if (row.kind === "subnet") return <IconKindSubnet />;
  return <IconKindDomain />;
}

function IconSlot({ label, children }: { label: string; children: ReactNode }) {
  return (
    <span className="res-icon" role="img" aria-label={label} tabIndex={0}>
      {children}
      <span className="hover-note">{label}</span>
    </span>
  );
}

function sshHint(row: ResourceRow, tr: (key: string) => string, busy: boolean) {
  if (busy) return tr("resources.sshBusy");
  if (row.ssh?.state === "pending_host") return tr("resources.sshPendingHost");
  if (row.ssh?.state === "pending") return tr("resources.sshPending");
  if (row.ssh?.state === "failed") return tr("resources.sshFailed");
  if (!row.live) return tr("resources.offlineBanner");
  return tr("resources.ssh");
}

const ResourceRowView = memo(function ResourceRowView({
  row,
  tr,
  onCopy,
  onOpenDest,
  onSsh,
  onSshCopy,
  busy,
}: {
  row: ResourceRow;
  tr: (key: string) => string;
  onCopy: (text: string) => void;
  onOpenDest: (address: string) => void;
  onSsh: (row: ResourceRow) => void;
  onSshCopy: (row: ResourceRow) => void;
  busy: boolean;
}) {
  const sshBusy = busy;
  return (
    <div className={`res-row${row.live ? " is-live" : ""}`}>
      <span
        className="res-live"
        role="img"
        aria-label={row.live ? tr("resources.live") : undefined}
        title={row.live ? tr("resources.live") : undefined}
      />
      <div className="res-main">
        <span className="res-name">{row.address}</span>
        <span className="res-meta">
          <IconSlot label={tr(kindKeyOf(row))}>{kindIconOf(row)}</IconSlot>
          <span className="res-proto">
            <span className="res-proto-name">{row.protocol}</span>
            {row.ports ? <span className="res-ports">{row.ports}</span> : null}
          </span>
          <IconSlot label={tr(row.source === "group" ? "resources.sourceGroup" : "resources.sourceTicket")}>
            {row.source === "group" ? <IconSourceGroup /> : <IconSourceTicket />}
          </IconSlot>
          {row.pending && (
            <IconSlot label={tr("resources.pending")}>
              <IconPending />
            </IconSlot>
          )}
        </span>
      </div>
      <span className="res-actions">
        <button
          type="button"
          className="ico-btn"
          onClick={() => void onCopy(row.address)}
          aria-label={tr("resources.copy")}
        >
          <IconCopy />
          <IconCheck />
        </button>
        {row.ssh && (
          <>
            <button
              type="button"
              className="ico-btn"
              disabled={sshBusy || !row.live || row.ssh.state !== "issued"}
              title={tr("ssh.copyCommand")}
              onClick={() => void onSshCopy(row)}
              aria-label={tr("ssh.copyCommand")}
            >
              <IconCommandCopy />
            </button>
            <button
              type="button"
              className="ico-btn ssh"
              disabled={sshBusy || !row.live || row.ssh.state !== "issued"}
              title={sshHint(row, tr, sshBusy)}
              onClick={() => void onSsh(row)}
              aria-label={tr("resources.ssh")}
            >
              <IconTerminal />
            </button>
          </>
        )}
        {isLaunchable(row.address, row.kind) && (
          <button
            type="button"
            className="ico-btn launch"
            onClick={() => void onOpenDest(row.address)}
            aria-label={tr("resources.open")}
          >
            <IconLaunch />
          </button>
        )}
      </span>
    </div>
  );
});

export function HomePage({
  connection,
  connected,
  connecting,
  signedIn,
  logo,
  statusLabel,
  heroSub,
  orbCaption,
  overlayIp,
  hostname,
  userEmail,
  linkOk,
  copied,
  resourceSystems,
  resourcesBusy,
  tr,
  sshBusyKey,
  loginRequired,
  onSignIn,
  onToggleConnect,
  onCopy,
  onOpenDest,
  onSsh,
  onSshCopy,
  onRequestAccess,
  onRefreshAssets,
}: {
  connection: UiConn;
  connected: boolean;
  connecting: boolean;
  signedIn: boolean;
  logo: string;
  statusLabel: string;
  heroSub: string;
  orbCaption: string;
  overlayIp: string;
  hostname: string;
  userEmail: string;
  linkOk: boolean;
  copied: boolean;
  resourceSystems: ResourceSystem[];
  resourcesBusy: boolean;
  sshBusyKey: string | null;
  loginRequired: boolean;
  onSignIn: () => void;
  tr: (key: string) => string;
  onToggleConnect: () => void;
  onCopy: (text: string) => void;
  onOpenDest: (address: string) => void;
  onSsh: (row: ResourceRow) => void;
  onSshCopy: (row: ResourceRow) => void;
  onRequestAccess: () => void;
  onRefreshAssets: () => void;
}) {
  const [query, setQuery] = useState("");
  const [openKeys, setOpenKeys] = useState<Record<string, boolean>>({});
  const [resourceView, setResourceView] = useState<"grouped" | "flat">("grouped");

  const q = query.trim().toLowerCase();
  const systemLabel = (system: ResourceSystem) =>
    system.id === UNGROUPED_SYSTEM ? tr("resources.customSystem") : system.name;
  const rowHit = (row: ResourceRow) =>
    row.address.toLowerCase().includes(q) ||
    row.protocol.toLowerCase().includes(q) ||
    row.ports.toLowerCase().includes(q) ||
    tr(kindKeyOf(row)).toLowerCase().includes(q);

  const visibleSystems = useMemo(() => {
    if (!q) return resourceSystems;
    const out: ResourceSystem[] = [];
    for (const system of resourceSystems) {
      const systemHit = systemLabel(system).toLowerCase().includes(q);
      const assets = system.assets
        .map((asset) => {
          const assetHit = systemHit || asset.name.toLowerCase().includes(q);
          return { ...asset, rows: assetHit ? asset.rows : asset.rows.filter(rowHit) };
        })
        .filter((asset) => asset.rows.length > 0);
      const rows = systemHit ? system.rows : system.rows.filter(rowHit);
      if (!systemHit && assets.length === 0 && rows.length === 0) continue;
      out.push({ ...system, assets, rows });
    }
    return out;
  }, [resourceSystems, q, tr]);

  const visibleRows = useMemo(() => collectRows(visibleSystems), [visibleSystems]);
  const totalRows = useMemo(() => collectRows(resourceSystems).length, [resourceSystems]);
  const resourceCount = visibleRows.length;
  const showSyncing = connected && resourcesBusy && totalRows === 0 && !loginRequired;
  const showEmpty = !signedIn || (q ? visibleRows.length === 0 : totalRows === 0);
  const emptyTitle = loginRequired
    ? tr("resources.loginRequiredEmpty")
    : q && signedIn
      ? tr("resources.noMatch")
      : tr("resources.emptyTitle");
  const emptySub = loginRequired
    ? ""
    : q && signedIn
      ? tr("resources.noMatchSub")
      : tr("resources.emptySub");
  const isOpen = (key: string) => (q ? true : openKeys[key] !== false);
  const toggle = (key: string) => setOpenKeys((prev) => ({ ...prev, [key]: prev[key] === false }));

  return (
    <section className="main-split">
      <aside className="connect-pane">
        <div className="hero-connect">
          <span className="halo-outer" aria-hidden="true" />
          <span className="halo-inner" aria-hidden="true" />
          <button
            type="button"
            className={`connect-orb${
              connecting || connection === "reconnecting" ? " is-connecting" : ""
            }`}
            aria-pressed={connected}
            aria-label={orbCaption}
            disabled={connecting}
            onClick={onToggleConnect}
          >
            <img className="connect-logo" src={logo} width={64} height={64} alt="" />
            <span className="orb-caption">{orbCaption}</span>
          </button>
        </div>
        <div className="connect-status">
          <span className="status-dot" aria-hidden="true" />
          {statusLabel}
        </div>
        <div className={`connect-sub${connected && linkOk ? " is-ok" : ""}`}>{heroSub}</div>
        {connection === "error" && (
          <button type="button" className="btn btn-primary btn-sm btn-retry" onClick={onToggleConnect}>
            {tr("action.retry")}
          </button>
        )}
        <div className="connect-meta">
          <div className="meta-card">
            <div className="meta-head">
              <span className="meta-k">
                <IconIp />
                <span className="meta-label">{tr("field.overlayIp")}</span>
              </span>
              <button
                type="button"
                className={`ico-btn${copied ? " is-ok" : ""}`}
                disabled={!connected || !overlayIp}
                onClick={() => void onCopy(overlayIp)}
                aria-label={tr("resources.copy")}
              >
                <IconCopy />
                <IconCheck />
              </button>
            </div>
            <div className="meta-body">
              <strong>{connected ? overlayIp || "—" : tr("hero.ipPending")}</strong>
            </div>
          </div>
          <div className="meta-card">
            <div className="meta-head">
              <span className="meta-k">
                <IconHost />
                <span className="meta-label">{tr("field.hostname")}</span>
              </span>
              <button
                type="button"
                className="ico-btn"
                disabled={!hostname || hostname === "—"}
                onClick={() => void onCopy(hostname)}
                aria-label={tr("resources.copy")}
              >
                <IconCopy />
                <IconCheck />
              </button>
            </div>
            <div className="meta-body">
              <strong>{hostname}</strong>
            </div>
          </div>
          <div className="meta-card">
            <div className="meta-head">
              <span className="meta-k">
                <IconUser />
                <span className="meta-label">{tr("field.loginUser")}</span>
              </span>
              <button
                type="button"
                className="ico-btn"
                disabled={!userEmail || userEmail === "—"}
                onClick={() => void onCopy(userEmail)}
                aria-label={tr("resources.copy")}
              >
                <IconCopy />
                <IconCheck />
              </button>
            </div>
            <div className="meta-body">
              <strong>{userEmail}</strong>
            </div>
          </div>
        </div>
      </aside>

      <div className="right-column">
        <div className={`resources-pane${signedIn && !connected ? " is-offline" : ""}`}>
          <div className="panel-head">
            <div className="panel-title">
              <h3>{tr("resources.title")}</h3>
              {resourceCount > 0 && <span className="count-badge">{resourceCount}</span>}
            </div>
            <div className="panel-tools">
              <label className="search-field compact" htmlFor="search-res">
                <IconSearch />
                <input
                  id="search-res"
                  type="search"
                  placeholder={tr("resources.search")}
                  value={query}
                  onChange={(e) => setQuery(e.target.value)}
                  autoComplete="off"
                />
                {query && (
                  <button type="button" className="search-clear" onClick={() => setQuery("")} aria-label={tr("resources.clearSearch")}>
                    <IconClear />
                  </button>
                )}
              </label>
              <div className="view-toggle" role="group" aria-label={tr("resources.viewMode")}>
                <button
                  type="button"
                  className={`view-toggle-btn${resourceView === "grouped" ? " is-active" : ""}`}
                  aria-pressed={resourceView === "grouped"}
                  aria-label={tr("resources.groupedView")}
                  title={tr("resources.groupedView")}
                  onClick={() => setResourceView("grouped")}
                >
                  <IconFolderTree />
                </button>
                <button
                  type="button"
                  className={`view-toggle-btn${resourceView === "flat" ? " is-active" : ""}`}
                  aria-pressed={resourceView === "flat"}
                  aria-label={tr("resources.flatView")}
                  title={tr("resources.flatView")}
                  onClick={() => setResourceView("flat")}
                >
                  <IconList />
                </button>
              </div>
              <button
                type="button"
                className="ico-btn"
                disabled={resourcesBusy}
                onClick={onRefreshAssets}
                aria-label={tr("resources.refresh")}
                title={tr("resources.refresh")}
              >
                <IconRefresh />
              </button>
            </div>
          </div>
          {signedIn && !connected && <div className="offline-banner">{tr("resources.offlineBanner")}</div>}
          {loginRequired && (
            <div className="login-banner" role="status">
              <span>{tr("home.loginRequired")}</span>
              <button type="button" className="btn btn-ghost" onClick={onSignIn}>
                {tr("action.signInSso")}
              </button>
            </div>
          )}
          {showSyncing ? (
            <div className="empty">
              <div className="empty-ico" aria-hidden="true">
                <IconEmptyLock />
              </div>
              <p>{tr("resources.syncing")}</p>
              <p className="muted">{tr("resources.syncingSub")}</p>
            </div>
          ) : showEmpty ? (
            <div className="empty">
              <div className="empty-ico" aria-hidden="true">
                <IconEmptyLock />
              </div>
              <p>{emptyTitle}</p>
              {emptySub && <p className="muted">{emptySub}</p>}
              {!loginRequired && (
                <button
                  type="button"
                  className="btn btn-ghost btn-refresh"
                  disabled={resourcesBusy}
                  onClick={onRefreshAssets}
                >
                  <IconRefresh />
                  <span>{tr("resources.refresh")}</span>
                </button>
              )}
            </div>
          ) : (
            resourceView === "grouped" ? (
              <div className="tree">
                {visibleSystems.map((system) => (
                  <div key={system.id} className={`asset${isOpen(system.id) ? " open" : ""}`}>
                    <button type="button" className="asset-head" onClick={() => toggle(system.id)}>
                      <span className="chev" aria-hidden="true">
                        <IconChevron />
                      </span>
                      <span className="asset-name">{systemLabel(system)}</span>
                    </button>
                    {isOpen(system.id) && (
                      <div className="asset-body">
                        {system.assets.map((asset) => (
                          <div key={asset.id} className={`asset-sub${isOpen(asset.id) ? " open" : ""}`}>
                            <button type="button" className="sub-head" onClick={() => toggle(asset.id)}>
                              <span className="chev" aria-hidden="true">
                                <IconChevron />
                              </span>
                              <span className="asset-name">{asset.name}</span>
                            </button>
                            {isOpen(asset.id) && (
                              <div className="sub-body">
                                {asset.rows.map((row) => (
                                  <ResourceRowView
                                    key={row.key}
                                    row={row}
                                    tr={tr}
                                    onCopy={onCopy}
                                    onOpenDest={onOpenDest}
                                    onSsh={onSsh}
                                    onSshCopy={onSshCopy}
                                    busy={sshBusyKey === row.key}
                                  />
                                ))}
                              </div>
                            )}
                          </div>
                        ))}
                        {system.rows.map((row) => (
                          <ResourceRowView
                            key={row.key}
                            row={row}
                            tr={tr}
                            onCopy={onCopy}
                            onOpenDest={onOpenDest}
                            onSsh={onSsh}
                            onSshCopy={onSshCopy}
                            busy={sshBusyKey === row.key}
                          />
                        ))}
                      </div>
                    )}
                  </div>
                ))}
              </div>
            ) : (
              <div className="service-list">
                {visibleRows.map((row) => (
                  <ResourceRowView
                    key={row.key}
                    row={row}
                    tr={tr}
                    onCopy={onCopy}
                    onOpenDest={onOpenDest}
                    onSsh={onSsh}
                    onSshCopy={onSshCopy}
                    busy={sshBusyKey === row.key}
                  />
                ))}
              </div>
            )
          )}
        </div>
        <div className="request-pane">
          <span className="request-ico" aria-hidden="true">
            <IconStar />
          </span>
          <div className="request-copy">
            <h4>{tr("guide.title")}</h4>
            <p className="guide-body">{tr("guide.body")}</p>
          </div>
          <button
            type="button"
            className="btn btn-contrast btn-request"
            onClick={onRequestAccess}
          >
            <span>{tr("action.requestAccess")}</span>
            <IconArrow />
          </button>
        </div>
      </div>
    </section>
  );
}
