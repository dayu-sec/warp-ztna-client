import { IconWarn } from "../Icons";

export function SessionOverlay({
  title,
  body,
  renewLabel,
  laterLabel,
  disconnectLabel,
  onRenew,
  onLater,
  onDisconnect,
}: {
  title: string;
  body: string;
  renewLabel: string;
  laterLabel: string;
  disconnectLabel: string;
  onRenew: () => void;
  onLater: () => void;
  onDisconnect: () => void;
}) {
  return (
    <div className="overlay">
      <div className="modal">
        <div className="modal-icon warn" aria-hidden="true">
          <IconWarn />
        </div>
        <h3>{title}</h3>
        <p className="muted">{body}</p>
        <div className="form-actions split">
          <button type="button" className="btn btn-ghost" onClick={onDisconnect}>
            {disconnectLabel}
          </button>
          <button type="button" className="btn btn-ghost" onClick={onLater}>
            {laterLabel}
          </button>
          <button type="button" className="btn btn-primary" onClick={onRenew}>
            {renewLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
