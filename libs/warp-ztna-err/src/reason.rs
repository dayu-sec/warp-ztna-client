use orion_error::{OrionError, UnifiedReason};

/// Stable operation-level reasons for Warp ZTNA control plane and agent paths.
///
/// Domain crates keep their own `thiserror` enums; applications lift those into
/// [`Error`](crate::Error) with a [`Reason`] via `or_err` / `because`.
#[derive(Debug, Clone, PartialEq, OrionError)]
pub enum Reason {
    #[orion_error(identity = "sys.internal")]
    Internal,
    #[orion_error(identity = "biz.invalid_argument")]
    InvalidArgument,
    #[orion_error(identity = "biz.not_found")]
    NotFound,
    #[orion_error(identity = "biz.conflict")]
    Conflict,
    #[orion_error(identity = "biz.unauthorized")]
    Unauthorized,
    #[orion_error(identity = "biz.forbidden")]
    Forbidden,
    #[orion_error(identity = "sys.unavailable")]
    Unavailable,
    #[orion_error(identity = "sys.timeout")]
    Timeout,
    /// Failure attributed to an external system (`NetBird`, asset center, `IdP`, …).
    #[orion_error(identity = "sys.external")]
    External,
    /// Asset graph failed L-constraints or schema checks.
    #[orion_error(identity = "biz.invalid_asset_graph")]
    InvalidAssetGraph,
    #[orion_error(transparent)]
    General(UnifiedReason),
}

impl Reason {
    /// Machine-stable identity string (e.g. `biz.not_found`).
    #[must_use]
    pub fn code(&self) -> &'static str {
        orion_error::reason::ErrorIdentityProvider::stable_code(self)
    }
}
