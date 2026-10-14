//! Errors when loading a pack (spec §11.2).

/// Why a pack could not be loaded.
#[derive(Debug, thiserror::Error)]
pub enum PackError {
    /// Reading the file failed.
    #[error("could not read pack file")]
    Io(#[from] std::io::Error),
    /// The file does not start with `RLPK`: not a Rustline pack.
    #[error("not a Rustline pack (bad magic bytes)")]
    BadMagic,
    /// The pack was written by a different (usually newer) format version.
    #[error("pack format version {found} is not supported (this build reads version {supported})")]
    UnsupportedVersion {
        /// Version in the file.
        found: u16,
        /// Version this build understands.
        supported: u16,
    },
    /// The file is truncated or damaged.
    #[error("pack file is corrupt: {0}")]
    Corrupt(String),
}
