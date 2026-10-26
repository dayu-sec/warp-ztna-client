export function SsoOverlay({
  message,
  url,
  error,
  copyLabel,
  cancelLabel,
  retryLabel,
  onCopy,
  onCancel,
  onRetry,
}: {
  message: string;
  url: string;
  error?: string;
  copyLabel: string;
  cancelLabel: string;
  retryLabel?: string;
  onCopy: (text: string) => void;
  onCancel: () => void;
  onRetry?: () => void;
}) {
  return (
    <div className="overlay">
      <div className={error ? "modal modal-sso modal-sso-error" : "modal modal-sso"}>
        {error ? (
          <div className="modal-sso-mark" aria-hidden="true">
            !
          </div>
        ) : (
          <div className="spinner" />
        )}
        <p className="modal-sso-message">{message}</p>
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
