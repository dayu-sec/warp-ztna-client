export function ProfileDialog({
  title,
  hint,
  name,
  url,
  nameLabel,
  urlLabel,
  namePh,
  urlPh,
  cancelLabel,
  saveLabel,
  onName,
  onUrl,
  onCancel,
  onSave,
}: {
  title: string;
  hint: string;
  name: string;
  url: string;
  nameLabel: string;
  urlLabel: string;
  namePh: string;
  urlPh: string;
  cancelLabel: string;
  saveLabel: string;
  onName: (value: string) => void;
  onUrl: (value: string) => void;
  onCancel: () => void;
  onSave: () => void;
}) {
  return (
    <div className="overlay overlay-form">
      <div className="profile-dialog" role="dialog">
        <div className="profile-dialog-head">
          <h3>{title}</h3>
          <p>{hint}</p>
        </div>
        <div className="profile-dialog-body">
          <label className="field">
            <span>{nameLabel}</span>
            <input value={name} onChange={(e) => onName(e.target.value)} placeholder={namePh} />
          </label>
          <label className="field">
            <span>{urlLabel}</span>
            <input value={url} onChange={(e) => onUrl(e.target.value)} placeholder={urlPh} />
          </label>
        </div>
        <div className="profile-dialog-foot">
          <button type="button" className="btn btn-ghost" onClick={onCancel}>
            {cancelLabel}
          </button>
          <button type="button" className="btn btn-primary" onClick={onSave}>
            {saveLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
