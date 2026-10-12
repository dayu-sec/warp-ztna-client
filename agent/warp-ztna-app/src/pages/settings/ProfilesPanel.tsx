import { IconEdit, IconPlus, IconProfiles, IconTrash } from "../../components/Icons";
import type { ProfileRow } from "../../types/agent";

export function ProfilesPanel({
  profiles,
  tr,
  onNew,
  onUse,
  onEdit,
  onDelete,
}: {
  profiles: ProfileRow[];
  tr: (key: string) => string;
  onNew: () => void;
  onUse: (p: ProfileRow) => void;
  onEdit: (p: ProfileRow) => void;
  onDelete: (p: ProfileRow) => void;
}) {
  return (
    <div className="s-panel active">
      <div className="panel-head-row">
        <header className="s-intro">
          <h3>{tr("settings.profiles")}</h3>
          <p className="hint">{tr("profiles.hint")}</p>
        </header>
        <button type="button" className="btn btn-primary btn-sm" onClick={onNew}>
          <IconPlus />
          <span>{tr("profiles.new")}</span>
        </button>
      </div>
      <div className="profile-list">
        {profiles.map((p) => (
          <div key={p.id || p.name} className={`profile-item${p.is_active ? " current" : ""}`}>
            <span className="pf-ico" aria-hidden="true">
              <IconProfiles />
            </span>
            <div className="info">
              <div className="name">
                {p.name}
                {p.is_active && <span className="chip-type">{tr("profiles.current")}</span>}
              </div>
              <div className="url">{p.url}</div>
            </div>
            <div className="actions">
              {p.is_active ? (
                <span className="profile-connected">{tr("profiles.connected")}</span>
              ) : (
                <>
                  <button type="button" className="btn btn-ghost btn-sm" onClick={() => onUse(p)}>
                    {tr("action.use")}
                  </button>
                  <button
                    type="button"
                    className="ico-btn"
                    onClick={() => onEdit(p)}
                    aria-label={tr("action.edit")}
                    title={tr("action.edit")}
                  >
                    <IconEdit />
                  </button>
                  <button
                    type="button"
                    className="ico-btn danger"
                    onClick={() => onDelete(p)}
                    aria-label={tr("action.delete")}
                    title={tr("action.delete")}
                  >
                    <IconTrash />
                  </button>
                </>
              )}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
