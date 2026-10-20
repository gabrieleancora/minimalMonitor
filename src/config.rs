use serde::Deserialize;
use std::{
    fs::File,
    io::Read,
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf},
};
use url::Url;

use crate::error::{Error, Result};

const MAX_CONFIG_BYTES: u64 = 16 * 1024;

#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Development,
    Production,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    mode: Mode,
    listen: SocketAddr,
    state_dir: PathBuf,
    public_origin: String,
    trusted_proxy: Option<IpAddr>,
    #[serde(default)]
    destination_rules: Vec<crate::destination::Rule>,
    #[serde(default)]
    extra_trust_roots: Vec<PathBuf>,
    telegram_token_file: Option<PathBuf>,
}

/// Deliberately lacks Debug: configuration may contain sensitive hostnames.
pub struct Config {
    pub mode: Mode,
    pub listen: SocketAddr,
    pub state_dir: PathBuf,
    pub public_origin: Url,
    pub destination_policy: crate::destination::Policy,
    pub trusted_proxy: Option<IpAddr>,
    pub extra_trust_roots: Vec<tokio_rustls::rustls::pki_types::CertificateDer<'static>>,
    pub telegram_token: Option<std::sync::Arc<crate::notifications::Token>>,
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let mut text = String::new();
        File::open(path)
            .map_err(|_| Error::Configuration)?
            .take(MAX_CONFIG_BYTES + 1)
            .read_to_string(&mut text)
            .map_err(|_| Error::Configuration)?;
        if text.len() as u64 > MAX_CONFIG_BYTES {
            return Err(Error::Configuration);
        }
        let raw: RawConfig = toml::from_str(&text).map_err(|_| Error::Configuration)?;
        let base = path.parent().unwrap_or(Path::new("."));
        Self::validate(raw, base)
    }

    fn validate(raw: RawConfig, base: &Path) -> Result<Self> {
        let origin = Url::parse(&raw.public_origin).map_err(|_| Error::Configuration)?;
        let scheme_ok = origin.scheme() == "https"
            || (raw.mode == Mode::Development
                && origin.scheme() == "http"
                && matches!(origin.host_str(), Some("localhost" | "127.0.0.1" | "[::1]")));
        if !raw.listen.ip().is_loopback()
            || raw.listen.port() == 0
            || !scheme_ok
            || raw.public_origin.len() > 2048
            || raw.public_origin.chars().any(char::is_control)
            || origin.host_str().is_none()
            || !origin.username().is_empty()
            || origin.password().is_some()
            || origin.query().is_some()
            || origin.fragment().is_some()
            || origin.path() != "/"
            || origin.port() == Some(0)
            || raw.state_dir.as_os_str().is_empty()
            || raw.trusted_proxy.is_some_and(|ip| !ip.is_loopback())
        {
            return Err(Error::Configuration);
        }
        let path = if raw.state_dir.is_absolute() {
            raw.state_dir
        } else {
            base.join(raw.state_dir)
        };
        let state_dir = if path.is_absolute() {
            path
        } else {
            std::env::current_dir()
                .map_err(|_| Error::Configuration)?
                .join(path)
        };
        if raw.extra_trust_roots.len() > 16 {
            return Err(Error::Configuration);
        }
        let mut extra_trust_roots = Vec::with_capacity(raw.extra_trust_roots.len());
        let mut total = 0usize;
        for path in raw.extra_trust_roots {
            let path = if path.is_absolute() {
                path
            } else {
                base.join(path)
            };
            let mut bytes = Vec::new();
            File::open(path)
                .map_err(|_| Error::Configuration)?
                .take(16385)
                .read_to_end(&mut bytes)
                .map_err(|_| Error::Configuration)?;
            total += bytes.len();
            if bytes.is_empty() || bytes.len() > 16384 || total > 128 * 1024 {
                return Err(Error::Configuration);
            }
            let root = tokio_rustls::rustls::pki_types::CertificateDer::from(bytes);
            let mut test = tokio_rustls::rustls::RootCertStore::empty();
            test.add(root.clone()).map_err(|_| Error::Configuration)?;
            extra_trust_roots.push(root);
        }
        let telegram_token = raw
            .telegram_token_file
            .map(|path| {
                let path = if path.is_absolute() {
                    path
                } else {
                    base.join(path)
                };
                crate::notifications::Token::load(&path).map(std::sync::Arc::new)
            })
            .transpose()?;
        Ok(Self {
            mode: raw.mode,
            listen: raw.listen,
            state_dir,
            public_origin: origin,
            destination_policy: crate::destination::Policy::new(raw.destination_rules)?,
            trusted_proxy: raw.trusted_proxy,
            extra_trust_roots,
            telegram_token,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(mode: Mode, listen: &str, origin: &str) -> RawConfig {
        RawConfig {
            mode,
            listen: listen.parse().unwrap(),
            state_dir: "state".into(),
            public_origin: origin.into(),
            destination_rules: Vec::new(),
            trusted_proxy: None,
            extra_trust_roots: Vec::new(),
            telegram_token_file: None,
        }
    }

    #[test]
    fn production_rejects_unsafe_bind_and_origins() {
        for origin in [
            "http://example.invalid",
            "https://user:password@example.invalid",
            "https://example.invalid/?token=x",
            "https://example.invalid/#x",
            "https://example.invalid/path",
            "https://example.invalid:0",
        ] {
            assert!(
                Config::validate(
                    raw(Mode::Production, "127.0.0.1:8080", origin),
                    Path::new(".")
                )
                .is_err()
            );
        }
        for listen in ["0.0.0.0:8080", "[::]:8080", "192.0.2.1:8080", "127.0.0.1:0"] {
            assert!(
                Config::validate(
                    raw(Mode::Production, listen, "https://example.invalid"),
                    Path::new(".")
                )
                .is_err()
            );
        }
        assert!(
            Config::validate(
                raw(Mode::Production, "[::1]:8080", "https://example.invalid"),
                Path::new(".")
            )
            .is_ok()
        );
    }

    #[test]
    fn bounded_config_rejects_unknown_fields_and_resolves_relative_paths() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let text = "mode='development'\nlisten='127.0.0.1:8080'\nstate_dir='state'\npublic_origin='http://localhost:8080'\n";
        std::fs::write(&path, text).unwrap();
        assert_eq!(
            Config::load(&path).unwrap().state_dir,
            dir.path().join("state")
        );
        std::fs::write(&path, format!("{text}insecure_tls=true\n")).unwrap();
        assert!(Config::load(&path).is_err());
        std::fs::write(&path, format!("{text}trusted_proxy='8.8.8.8'\n")).unwrap();
        assert!(Config::load(&path).is_err());
        std::fs::write(&path, format!("{text}trusted_proxy='127.0.0.1'\n[[destination_rules]]\ncidr='10.0.0.0/24'\nhost='home.invalid'\nports=[443]\nping=true\n")).unwrap();
        let configured = Config::load(&path).unwrap();
        assert!(configured.trusted_proxy.is_some());
        assert!(
            configured
                .destination_policy
                .authorize(
                    &crate::destination::Target::tcp("home.invalid", 443).unwrap(),
                    &["10.0.0.2".parse().unwrap()]
                )
                .is_ok()
        );
        assert!(!dir.path().join("state").exists());
        std::fs::write(&path, "x".repeat(MAX_CONFIG_BYTES as usize + 1)).unwrap();
        assert!(Config::load(&path).is_err());
        assert!(
            Config::validate(
                raw(
                    Mode::Development,
                    "127.0.0.1:8080",
                    "http://example.invalid"
                ),
                dir.path()
            )
            .is_err()
        );
    }
}
