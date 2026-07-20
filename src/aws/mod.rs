use chrono::Utc;
use configparser::ini::Ini;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

/// Global verbose flag. When true, kee prints diagnostic detail (AWS CLI
/// stderr, cache parse errors) to stderr. Default behaviour stays silent.
///
/// Used by the binary (`main.rs`); the library half of the crate doesn't
/// reference it, hence the allow.
#[allow(dead_code)]
pub static VERBOSE: AtomicBool = AtomicBool::new(false);

/// Resolve the user's home directory.
///
/// Prefers the `HOME` env var (Unix) or `USERPROFILE` (Windows) when set,
/// falling back to `dirs::home_dir()` otherwise. The fallback on Windows
/// calls a Win32 API that ignores environment overrides, which makes
/// integration tests that point `HOME`/`USERPROFILE` at a tempdir useless.
/// Honouring the env vars first restores parity with Unix behaviour and
/// is harmless in production: real users on Windows have `USERPROFILE`
/// set to their actual profile path anyway.
pub fn home_dir() -> Option<PathBuf> {
    if let Some(val) = std::env::var_os("HOME") {
        if !val.is_empty() {
            return Some(PathBuf::from(val));
        }
    }
    if cfg!(windows) {
        if let Some(val) = std::env::var_os("USERPROFILE") {
            if !val.is_empty() {
                return Some(PathBuf::from(val));
            }
        }
    }
    dirs::home_dir()
}

/// Read the verbose flag.
#[allow(dead_code)]
pub fn is_verbose() -> bool {
    VERBOSE.load(Ordering::Relaxed)
}

/// Print a diagnostic line to stderr, but only when --verbose is on.
#[allow(unused_macros)]
macro_rules! vlog {
    ($($arg:tt)*) => {
        if $crate::aws::is_verbose() {
            eprintln!(" [v] {}", format!($($arg)*));
        }
    };
}
#[allow(unused_imports)]
pub(crate) use vlog;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ProfileInfo {
    pub profile_name: String,
    pub sso_start_url: String,
    pub sso_region: String,
    pub sso_account_id: String,
    pub sso_role_name: String,
    pub session_name: String,
    #[serde(default)]
    pub production: bool,
}

#[allow(dead_code)]
#[derive(Clone)]
pub struct AwsManager {
    aws_config_file: PathBuf,
    sso_cache_dir: PathBuf,
}

#[allow(dead_code)]
impl AwsManager {
    pub fn new() -> io::Result<Self> {
        let home_dir = home_dir().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "\n [X] Could not find the AWS home directory\n",
            )
        })?;

        let aws_config_file = home_dir.join(".aws").join("config");
        let sso_cache_dir = home_dir.join(".aws").join("sso").join("cache");

        Ok(Self {
            aws_config_file,
            sso_cache_dir,
        })
    }

    /// Test-only constructor that lets callers point at synthetic paths.
    #[cfg(test)]
    pub(crate) fn new_with_paths(aws_config_file: PathBuf, sso_cache_dir: PathBuf) -> Self {
        Self {
            aws_config_file,
            sso_cache_dir,
        }
    }

    pub fn load_config(&self) -> io::Result<Ini> {
        if !self.aws_config_file.exists() {
            return Ok(Ini::new());
        }

        let content = fs::read_to_string(&self.aws_config_file)?;
        let mut config = Ini::new();
        config
            .read(content)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        Ok(config)
    }

    pub fn save_config(&self, config: &Ini) -> io::Result<()> {
        let mut output = String::new();
        for (section_name, section_map) in config.get_map_ref() {
            output.push_str(&format!("[{section_name}]\n"));
            for (key, value_opt) in section_map {
                if let Some(value) = value_opt {
                    output.push_str(&format!("{key} = {value}\n"));
                }
            }
            output.push('\n');
        }
        fs::write(&self.aws_config_file, output)
    }

    pub fn format_config(&self) -> io::Result<()> {
        let config = self.load_config()?;
        self.save_config(&config)
    }

    pub fn remove_profile(&self, profile_name: &str) -> io::Result<()> {
        let mut config = self.load_config()?;
        let section_name = format!("profile {profile_name}");
        config.remove_section(&section_name);
        self.save_config(&config)
    }

    pub fn read_profile(&self, profile_name: &str) -> Option<ProfileInfo> {
        if !self.aws_config_file.exists() {
            return None;
        }

        let content = fs::read_to_string(&self.aws_config_file).ok()?;
        let mut config = Ini::new();
        config.read(content).ok()?;

        let section_name = format!("profile {profile_name}");
        let section = config.get_map_ref().get(&section_name)?;

        let sso_account_id = section.get("sso_account_id")?.as_ref()?.clone();
        let sso_role_name = section.get("sso_role_name")?.as_ref()?.clone();

        let session_name = section
            .get("sso_session")
            .and_then(|s| s.as_ref())
            .unwrap_or(&String::new())
            .clone();

        // Helper function to get string value from section
        let get_value = |section: &HashMap<String, Option<String>>, key: &str| {
            section
                .get(key)
                .and_then(|s| s.as_ref())
                .cloned()
                .unwrap_or_default()
        };

        // Handle SSO session format - get sso_start_url and sso_region from sso-session section
        let (sso_start_url, sso_region) = if !session_name.is_empty() {
            let sso_section_name = format!("sso-session {session_name}");
            if let Some(sso_section) = config.get_map_ref().get(&sso_section_name) {
                (
                    get_value(sso_section, "sso_start_url"),
                    get_value(sso_section, "sso_region"),
                )
            } else {
                (String::new(), String::new())
            }
        } else {
            // Legacy format - try to get from profile section
            (
                get_value(section, "sso_start_url"),
                get_value(section, "sso_region"),
            )
        };

        Some(ProfileInfo {
            profile_name: profile_name.to_string(),
            sso_start_url,
            sso_region,
            sso_account_id,
            sso_role_name,
            session_name,
            production: false,
        })
    }

    /// Read the expiry timestamp of the cached SSO token for the given profile.
    pub fn read_token_expiry(&self, profile_info: &ProfileInfo) -> Option<chrono::DateTime<Utc>> {
        let cache_file = self.find_sso_cache_file(profile_info)?;
        let content = fs::read_to_string(&cache_file).ok()?;
        let cache: SsoTokenCache = serde_json::from_str(&content).ok()?;
        cache.expires_at?.parse::<chrono::DateTime<Utc>>().ok()
    }

    /// Determine the health of a profile's SSO session from its cached token.
    ///
    /// A profile is not "expired" just because its access token has lapsed: the
    /// AWS CLI and SDKs refresh the access token on demand as long as the
    /// refresh token and client registration are still valid, and cached role
    /// credentials keep working in the meantime. So we report three states: a
    /// live access token (`Active`), a lapsed access token that will still
    /// auto-refresh (`Refreshable`), and a genuinely dead session that needs a
    /// fresh `aws sso login` (`Expired`).
    pub fn read_session_health(&self, profile_info: &ProfileInfo) -> SessionHealth {
        let cache_file = match self.find_sso_cache_file(profile_info) {
            Some(p) => p,
            None => return SessionHealth::Expired,
        };
        let content = match fs::read_to_string(&cache_file) {
            Ok(c) => c,
            Err(_) => return SessionHealth::Expired,
        };
        let cache: SsoTokenCache = match serde_json::from_str(&content) {
            Ok(c) => c,
            Err(_) => return SessionHealth::Expired,
        };

        let now = Utc::now();

        // Access token still valid: the common healthy case.
        if let Some(exp) = cache
            .expires_at
            .as_deref()
            .and_then(|s| s.parse::<chrono::DateTime<Utc>>().ok())
        {
            if exp > now {
                return SessionHealth::Active(exp);
            }
        }

        // Access token has lapsed. The session is still alive if a refresh
        // token is present and the client registration hasn't expired, since
        // the CLI/SDK will silently mint a new access token on next use.
        let has_refresh = cache
            .refresh_token
            .as_deref()
            .is_some_and(|s| !s.is_empty());
        let registration_live = cache
            .registration_expires_at
            .as_deref()
            .and_then(|s| s.parse::<chrono::DateTime<Utc>>().ok())
            .is_some_and(|r| r > now);

        if has_refresh && registration_live {
            SessionHealth::Refreshable
        } else {
            SessionHealth::Expired
        }
    }

    /// Find the SSO cache file for a given profile by matching the start URL or session name.
    fn find_sso_cache_file(&self, profile_info: &ProfileInfo) -> Option<PathBuf> {
        if !self.sso_cache_dir.exists() {
            return None;
        }

        let entries = fs::read_dir(&self.sso_cache_dir).ok()?;

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }

            let content = match fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let cache: SsoTokenCache = match serde_json::from_str(&content) {
                Ok(c) => c,
                Err(_) => continue,
            };

            // Match by start URL — this is how the AWS CLI identifies cache entries
            if let Some(ref url) = cache.start_url {
                if url == &profile_info.sso_start_url {
                    return Some(path);
                }
            }
        }

        None
    }
}

/// Health of a profile's SSO session, derived from its cached token. This is
/// distinct from "can I read an expiry timestamp": a lapsed access token with a
/// valid refresh token is still a working session.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
pub enum SessionHealth {
    /// Access token is valid until the given instant.
    Active(chrono::DateTime<Utc>),
    /// Access token has lapsed, but a refresh token and a live client
    /// registration remain, so the CLI/SDK will refresh on demand.
    Refreshable,
    /// Nothing usable is cached; a full `aws sso login` is required.
    Expired,
}

/// Represents the cached SSO token file in ~/.aws/sso/cache/
#[allow(dead_code)]
#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct SsoTokenCache {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registration_expires_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Build a ProfileInfo pointing at the given start URL. Other fields are
    /// filler; they aren't checked by the cache-finding logic.
    fn profile(start_url: &str) -> ProfileInfo {
        ProfileInfo {
            profile_name: "test".into(),
            sso_start_url: start_url.into(),
            sso_region: "ap-southeast-2".into(),
            sso_account_id: "123456789012".into(),
            sso_role_name: "TestRole".into(),
            session_name: "test-session".into(),
            production: false,
        }
    }

    /// Write a JSON file representing one entry in `~/.aws/sso/cache`.
    fn write_cache_file(dir: &std::path::Path, name: &str, body: &str) {
        fs::write(dir.join(name), body).unwrap();
    }

    fn manager_with_cache(cache_dir: &std::path::Path) -> AwsManager {
        AwsManager::new_with_paths(
            // aws_config_file isn't exercised by these tests; point at a
            // sibling path that doesn't exist.
            cache_dir.parent().unwrap().join("config"),
            cache_dir.to_path_buf(),
        )
    }

    // -- find_sso_cache_file --------------------------------------------------

    #[test]
    fn find_sso_cache_returns_none_when_dir_missing() {
        let tmp = TempDir::new().unwrap();
        let mgr = manager_with_cache(&tmp.path().join("does-not-exist"));
        assert!(mgr
            .find_sso_cache_file(&profile("https://acme.awsapps.com/start"))
            .is_none());
    }

    #[test]
    fn find_sso_cache_matches_by_start_url() {
        let tmp = TempDir::new().unwrap();
        let cache_dir = tmp.path().join("cache");
        fs::create_dir_all(&cache_dir).unwrap();
        write_cache_file(
            &cache_dir,
            "abc.json",
            r#"{"startUrl":"https://acme.awsapps.com/start","accessToken":"t"}"#,
        );
        write_cache_file(
            &cache_dir,
            "def.json",
            r#"{"startUrl":"https://other.awsapps.com/start","accessToken":"t"}"#,
        );

        let mgr = manager_with_cache(&cache_dir);
        let found = mgr
            .find_sso_cache_file(&profile("https://acme.awsapps.com/start"))
            .expect("should match");
        assert!(found.ends_with("abc.json"));
    }

    #[test]
    fn find_sso_cache_returns_none_when_no_match() {
        let tmp = TempDir::new().unwrap();
        let cache_dir = tmp.path().join("cache");
        fs::create_dir_all(&cache_dir).unwrap();
        write_cache_file(
            &cache_dir,
            "abc.json",
            r#"{"startUrl":"https://acme.awsapps.com/start","accessToken":"t"}"#,
        );

        let mgr = manager_with_cache(&cache_dir);
        assert!(mgr
            .find_sso_cache_file(&profile("https://nope.awsapps.com/start"))
            .is_none());
    }

    #[test]
    fn find_sso_cache_tolerates_malformed_json() {
        let tmp = TempDir::new().unwrap();
        let cache_dir = tmp.path().join("cache");
        fs::create_dir_all(&cache_dir).unwrap();
        // Garbage file alongside a valid one. The lookup must skip the first
        // and find the second.
        write_cache_file(&cache_dir, "broken.json", "not json at all");
        write_cache_file(
            &cache_dir,
            "ok.json",
            r#"{"startUrl":"https://acme.awsapps.com/start","accessToken":"t"}"#,
        );

        let mgr = manager_with_cache(&cache_dir);
        let found = mgr
            .find_sso_cache_file(&profile("https://acme.awsapps.com/start"))
            .expect("should match the well-formed file");
        assert!(found.ends_with("ok.json"));
    }

    #[test]
    fn find_sso_cache_ignores_non_json_files() {
        let tmp = TempDir::new().unwrap();
        let cache_dir = tmp.path().join("cache");
        fs::create_dir_all(&cache_dir).unwrap();
        // Stray file that isn't JSON; the function looks at .json only.
        fs::write(cache_dir.join("README.txt"), "ignore me").unwrap();
        write_cache_file(
            &cache_dir,
            "ok.json",
            r#"{"startUrl":"https://acme.awsapps.com/start","accessToken":"t"}"#,
        );

        let mgr = manager_with_cache(&cache_dir);
        assert!(mgr
            .find_sso_cache_file(&profile("https://acme.awsapps.com/start"))
            .is_some());
    }

    // -- read_token_expiry ----------------------------------------------------

    #[test]
    fn read_token_expiry_parses_valid_timestamp() {
        let tmp = TempDir::new().unwrap();
        let cache_dir = tmp.path().join("cache");
        fs::create_dir_all(&cache_dir).unwrap();
        write_cache_file(
            &cache_dir,
            "ok.json",
            r#"{"startUrl":"https://acme.awsapps.com/start","accessToken":"t","expiresAt":"2099-01-01T00:00:00Z"}"#,
        );

        let mgr = manager_with_cache(&cache_dir);
        let expiry = mgr
            .read_token_expiry(&profile("https://acme.awsapps.com/start"))
            .expect("should parse");
        assert_eq!(expiry.format("%Y-%m-%d").to_string(), "2099-01-01");
    }

    #[test]
    fn read_token_expiry_returns_none_for_missing_field() {
        let tmp = TempDir::new().unwrap();
        let cache_dir = tmp.path().join("cache");
        fs::create_dir_all(&cache_dir).unwrap();
        write_cache_file(
            &cache_dir,
            "ok.json",
            r#"{"startUrl":"https://acme.awsapps.com/start","accessToken":"t"}"#,
        );

        let mgr = manager_with_cache(&cache_dir);
        assert!(mgr
            .read_token_expiry(&profile("https://acme.awsapps.com/start"))
            .is_none());
    }

    #[test]
    fn read_token_expiry_returns_none_for_unmatched_profile() {
        let tmp = TempDir::new().unwrap();
        let cache_dir = tmp.path().join("cache");
        fs::create_dir_all(&cache_dir).unwrap();
        // Cache file exists, but for a different start URL.
        write_cache_file(
            &cache_dir,
            "ok.json",
            r#"{"startUrl":"https://other.awsapps.com/start","accessToken":"t","expiresAt":"2099-01-01T00:00:00Z"}"#,
        );

        let mgr = manager_with_cache(&cache_dir);
        assert!(mgr
            .read_token_expiry(&profile("https://acme.awsapps.com/start"))
            .is_none());
    }

    #[test]
    fn read_token_expiry_returns_none_for_malformed_timestamp() {
        let tmp = TempDir::new().unwrap();
        let cache_dir = tmp.path().join("cache");
        fs::create_dir_all(&cache_dir).unwrap();
        write_cache_file(
            &cache_dir,
            "ok.json",
            r#"{"startUrl":"https://acme.awsapps.com/start","accessToken":"t","expiresAt":"not a date"}"#,
        );

        let mgr = manager_with_cache(&cache_dir);
        assert!(mgr
            .read_token_expiry(&profile("https://acme.awsapps.com/start"))
            .is_none());
    }

    // -- read_session_health --------------------------------------------------

    #[test]
    fn session_health_active_when_access_token_valid() {
        let tmp = TempDir::new().unwrap();
        let cache_dir = tmp.path().join("cache");
        fs::create_dir_all(&cache_dir).unwrap();
        write_cache_file(
            &cache_dir,
            "ok.json",
            r#"{"startUrl":"https://acme.awsapps.com/start","accessToken":"t","expiresAt":"2099-01-01T00:00:00Z"}"#,
        );

        let mgr = manager_with_cache(&cache_dir);
        assert!(matches!(
            mgr.read_session_health(&profile("https://acme.awsapps.com/start")),
            SessionHealth::Active(_)
        ));
    }

    #[test]
    fn session_health_refreshable_when_expired_but_refresh_token_live() {
        // Access token in the past, but a refresh token and a live client
        // registration remain: the session still auto-refreshes on use.
        let tmp = TempDir::new().unwrap();
        let cache_dir = tmp.path().join("cache");
        fs::create_dir_all(&cache_dir).unwrap();
        write_cache_file(
            &cache_dir,
            "ok.json",
            r#"{"startUrl":"https://acme.awsapps.com/start","accessToken":"t","expiresAt":"2000-01-01T00:00:00Z","refreshToken":"r","registrationExpiresAt":"2099-01-01T00:00:00Z"}"#,
        );

        let mgr = manager_with_cache(&cache_dir);
        assert_eq!(
            mgr.read_session_health(&profile("https://acme.awsapps.com/start")),
            SessionHealth::Refreshable
        );
    }

    #[test]
    fn session_health_expired_when_lapsed_and_no_refresh_token() {
        let tmp = TempDir::new().unwrap();
        let cache_dir = tmp.path().join("cache");
        fs::create_dir_all(&cache_dir).unwrap();
        write_cache_file(
            &cache_dir,
            "ok.json",
            r#"{"startUrl":"https://acme.awsapps.com/start","accessToken":"t","expiresAt":"2000-01-01T00:00:00Z"}"#,
        );

        let mgr = manager_with_cache(&cache_dir);
        assert_eq!(
            mgr.read_session_health(&profile("https://acme.awsapps.com/start")),
            SessionHealth::Expired
        );
    }

    #[test]
    fn session_health_expired_when_registration_also_lapsed() {
        // A refresh token is present but the client registration has expired,
        // so a refresh would fail: treat the session as dead.
        let tmp = TempDir::new().unwrap();
        let cache_dir = tmp.path().join("cache");
        fs::create_dir_all(&cache_dir).unwrap();
        write_cache_file(
            &cache_dir,
            "ok.json",
            r#"{"startUrl":"https://acme.awsapps.com/start","accessToken":"t","expiresAt":"2000-01-01T00:00:00Z","refreshToken":"r","registrationExpiresAt":"2000-06-01T00:00:00Z"}"#,
        );

        let mgr = manager_with_cache(&cache_dir);
        assert_eq!(
            mgr.read_session_health(&profile("https://acme.awsapps.com/start")),
            SessionHealth::Expired
        );
    }

    #[test]
    fn session_health_expired_when_no_cache_file() {
        let tmp = TempDir::new().unwrap();
        let mgr = manager_with_cache(&tmp.path().join("cache"));
        assert_eq!(
            mgr.read_session_health(&profile("https://acme.awsapps.com/start")),
            SessionHealth::Expired
        );
    }

    // -- ProfileInfo round-trip with production flag --------------------------

    #[test]
    fn profile_info_serialises_production_flag() {
        let p = ProfileInfo {
            profile_name: "prod".into(),
            sso_start_url: "https://acme.awsapps.com/start".into(),
            sso_region: "ap-southeast-2".into(),
            sso_account_id: "123456789012".into(),
            sso_role_name: "Admin".into(),
            session_name: "acme".into(),
            production: true,
        };
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains("\"production\":true"));

        let back: ProfileInfo = serde_json::from_str(&json).unwrap();
        assert!(back.production);
    }

    #[test]
    fn profile_info_defaults_production_when_missing() {
        // Older configs (before the production flag was added) won't have the
        // field. Deserialising must default it to false rather than failing.
        let json = r#"{
            "profile_name": "legacy",
            "sso_start_url": "https://acme.awsapps.com/start",
            "sso_region": "ap-southeast-2",
            "sso_account_id": "123456789012",
            "sso_role_name": "Admin",
            "session_name": "acme"
        }"#;
        let p: ProfileInfo = serde_json::from_str(json).unwrap();
        assert!(!p.production);
    }
}
