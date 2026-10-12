export function WelcomePage({
  logo,
  title,
  subtitle,
  actionLabel,
  hint,
  notice,
  profileLabel,
  profileName,
  onAction,
}: {
  logo: string;
  title: string;
  subtitle: string;
  actionLabel: string;
  hint: string;
  notice?: string;
  profileLabel?: string;
  profileName?: string;
  onAction: () => void;
}) {
  return (
    <section className="main-full">
      <div className="welcome-card">
        <div className="welcome-brand">
          <div className="welcome-logo-wrap">
            <span className="logo-halo" aria-hidden="true" />
            <img src={logo} width={72} height={72} alt="" />
          </div>
          <h2>{title}</h2>
          <p className="welcome-sub">{subtitle}</p>
        </div>
        <button type="button" className="btn btn-primary btn-lg welcome-cta" onClick={onAction}>
          {actionLabel}
        </button>
        <p className="hint welcome-foot">{hint}</p>
        {notice ? (
          <p className="welcome-notice" role="status">
            {notice}
          </p>
        ) : null}
        {profileName ? (
          <p className="welcome-profile">
            <span>{profileLabel}</span>
            <span className="welcome-profile-name">{profileName}</span>
          </p>
        ) : null}
      </div>
    </section>
  );
}
