export function SsoOverlay({
  message,
  url,
  copyLabel,
  cancelLabel,
  retryLabel,
  onCopy,
  onCancel,
  onRetry,
}: {
  message: string;
  url: string;
  copyLabel: string;
  cancelLabel: string;
  retryLabel?: string;
  onCopy: (text: string) => void;
  onCancel: () => void;
  onRetry?: () => void;
}) {
  return (
    <div className="overlay">
      <div className="modal modal-sso">
        <div className="spinner" />
        <p>{message}</p>
        {(retryLabel && onRetry) || url ? (
          <div className="modal-actions">
            {retryLabel && onRetry ? (
              <button type="button" className="btn btn-ghost" onClick={onRetry}>
                {retryLabel}
              </button>
            ) : null}
            {url ? (
              <button type="button" className="btn btn-ghost" onClick={() => onCopy(url)}>
                {copyLabel}
              </button>
            ) : null}
          </div>
        ) : null}
        <button type="button" className="btn btn-ghost" onClick={onCancel}>
          {cancelLabel}
        </button>
      </div>
    </div>
  );
}
