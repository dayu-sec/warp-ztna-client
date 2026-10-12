use std::fmt;

/// Context string that prefers static storage and owns only when necessary.
///
/// Facade API accepts `impl Into<ImmutStr>`. Values are converted to `String`
/// when attached to the underlying orion carrier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImmutStr {
    Static(&'static str),
    Owned(Box<str>),
}

impl ImmutStr {
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Static(value) => value,
            Self::Owned(value) => value.as_ref(),
        }
    }

    #[must_use]
    pub fn is_owned(&self) -> bool {
        matches!(self, Self::Owned(_))
    }

    #[must_use]
    pub fn into_string(self) -> String {
        match self {
            Self::Static(value) => value.to_owned(),
            Self::Owned(value) => value.into_string(),
        }
    }
}

impl fmt::Display for ImmutStr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<&'static str> for ImmutStr {
    fn from(value: &'static str) -> Self {
        Self::Static(value)
    }
}

impl From<String> for ImmutStr {
    fn from(value: String) -> Self {
        Self::Owned(value.into_boxed_str())
    }
}

impl From<Box<str>> for ImmutStr {
    fn from(value: Box<str>) -> Self {
        Self::Owned(value)
    }
}

impl AsRef<str> for ImmutStr {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
