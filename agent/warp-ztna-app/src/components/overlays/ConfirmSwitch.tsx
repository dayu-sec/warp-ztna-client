export function ConfirmSwitch({
  title,
  okLabel,
  cancelLabel,
  onOk,
  onCancel,
}: {
  title: string;
  okLabel: string;
  cancelLabel: string;
  onOk: () => void;
  onCancel: () => void;
}) {
  return (
    <div className="overlay">
      <div className="modal">
        <h3>{title}</h3>
        <div className="form-actions">
          <button type="button" className="btn btn-primary" onClick={onOk}>
            {okLabel}
          </button>
          <button type="button" className="btn btn-ghost" onClick={onCancel}>
            {cancelLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
