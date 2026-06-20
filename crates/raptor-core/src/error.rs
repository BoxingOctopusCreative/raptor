use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("package not found: {0}")]
    PackageNotFound(String),

    #[error("dependency conflict: {0}")]
    DependencyConflict(String),

    #[error("invalid control file: {0}")]
    InvalidControl(String),

    #[error("invalid deb archive: {0}")]
    InvalidDeb(String),

    #[error("invalid sources list: {0}")]
    InvalidSources(String),

    #[error("unsupported compression: {0}")]
    UnsupportedCompression(String),

    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("YAML error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("invalid PPA: {0}")]
    InvalidPpa(String),

    #[error("PPA already exists: {0}")]
    PpaExists(String),

    #[error("PPA fetch failed: {0}")]
    PpaFetch(String),

    #[error("remote fetch failed: {0}")]
    RemoteFetch(String),

    #[error("signature verification failed: {0}")]
    SignatureVerification(String),

    #[error("invalid Release file: {0}")]
    InvalidRelease(String),

    #[error("checksum verification failed: {0}")]
    ChecksumMismatch(String),

    #[error("refusing insecure repository update: {0}")]
    InsecureRepository(String),

    #[error("failed to acquire package: {0}")]
    PackageAcquire(String),

    #[error("{0}")]
    Other(String),
}
