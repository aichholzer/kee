use chrono::Utc;
use configparser::ini::Ini;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};

/// Global verbose flag. When true, kee prints diagnostic detail (AWS CLI
/// stderr, refresh-attempt outcomes, cache parse errors) to stderr.
/// Default behaviour stays silent.
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
pub fn is_verbose() -> bool {
    VERBOSE.load(Ordering::Relaxed)
}

/// Print a diagnostic line to stderr, but only when --verbose is on.
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

    /// Attempt to refresh an expired SSO access token using the cached refresh token.
    /// Returns true if the token was successfully refreshed (or if another process
    /// already refreshed it), false only when the token is genuinely dead.
    pub fn try_refresh_token(&self, profile_info: &ProfileInfo) -> bool {
        if self.do_refresh_token(profile_info).is_some() {
            return true;
        }

        // Refresh failed. Another process (AWS CLI, SDK, SOPS, etc.) may have
        // already rotated the refresh token, invalidating ours. Re-read the
        // cache: if the token is still valid, treat it as success.
        self.read_token_expiry(profile_info)
            .map(|expires_at| expires_at > chrono::Utc::now())
            .unwrap_or(false)
    }

    fn do_refresh_token(&self, profile_info: &ProfileInfo) -> Option<()> {
        let cache_file = match self.find_sso_cache_file(profile_info) {
            Some(p) => p,
            None => {
                vlog!(
                    "no SSO cache file matching start_url '{}'",
                    profile_info.sso_start_url
                );
                return None;
            }
        };
        let content = match fs::read_to_string(&cache_file) {
            Ok(c) => c,
            Err(e) => {
                vlog!("read {}: {}", cache_file.display(), e);
                return None;
            }
        };
        let cache: SsoTokenCache = match serde_json::from_str(&content) {
            Ok(c) => c,
            Err(e) => {
                vlog!("parse {}: {}", cache_file.display(), e);
                return None;
            }
        };

        let refresh_token = cache.refresh_token.as_ref().filter(|t| !t.is_empty())?;
        let client_id = cache.client_id.as_ref().filter(|id| !id.is_empty())?;
        let client_secret = cache.client_secret.as_ref().filter(|s| !s.is_empty())?;

        // Check that the client registration itself hasn't expired
        if let Some(ref reg_expires) = cache.registration_expires_at {
            let expires = reg_expires.parse::<chrono::DateTime<Utc>>().ok()?;
            if Utc::now() >= expires {
                vlog!("client registration expired at {reg_expires}");
                return None;
            }
        }

        // Call sso-oidc create-token with the refresh token
        let output = Command::new("aws")
            .args([
                "sso-oidc",
                "create-token",
                "--client-id",
                client_id,
                "--client-secret",
                client_secret,
                "--grant-type",
                "refresh_token",
                "--refresh-token",
                refresh_token,
                "--region",
                &profile_info.sso_region,
            ])
            .env("AWS_CLI_AUTO_PROMPT", "off")
            .env("AWS_PAGER", "")
            .output()
            .ok()?;

        if !output.status.success() {
            vlog!(
                "aws sso-oidc create-token failed (status {}): {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            );
            return None;
        }

        let response: CreateTokenResponse = match serde_json::from_slice(&output.stdout) {
            Ok(r) => r,
            Err(e) => {
                vlog!("parse create-token response: {e}");
                return None;
            }
        };

        // Build the updated cache and write it back
        let expires_at =
            Utc::now() + chrono::Duration::seconds(response.expires_in.unwrap_or(28800) as i64);

        let updated = SsoTokenCache {
            access_token: Some(response.access_token),
            expires_at: Some(expires_at.format("%Y-%m-%dT%H:%M:%SZ").to_string()),
            refresh_token: response.refresh_token.or(cache.refresh_token),
            client_id: cache.client_id,
            client_secret: cache.client_secret,
            registration_expires_at: cache.registration_expires_at,
            start_url: cache.start_url,
            region: cache.region,
        };

        let json = serde_json::to_string_pretty(&updated).ok()?;

        // Write atomically: tmp file + rename. A non-atomic fs::write truncates
        // the cache file before the new contents land, and the AWS CLI can read
        // it mid-write while minting role credentials.
        let tmp = cache_file.with_extension("json.tmp");
        if fs::write(&tmp, json).is_err() {
            let _ = fs::remove_file(&tmp);
            return None;
        }
        if fs::rename(&tmp, &cache_file).is_err() {
            let _ = fs::remove_file(&tmp);
            return None;
        }

        Some(())
    }

    /// Read the expiry timestamp of the cached SSO token for the given profile.
    pub fn read_token_expiry(&self, profile_info: &ProfileInfo) -> Option<chrono::DateTime<Utc>> {
        let cache_file = self.find_sso_cache_file(profile_info)?;
        let content = fs::read_to_string(&cache_file).ok()?;
        let cache: SsoTokenCache = serde_json::from_str(&content).ok()?;
        cache.expires_at?.parse::<chrono::DateTime<Utc>>().ok()
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

/// Response from `aws sso-oidc create-token`
#[allow(dead_code)]
#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct CreateTokenResponse {
    pub access_token: String,
    #[serde(default)]
    pub expires_in: Option<i64>,
    #[serde(default)]
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

    // -- try_refresh_token / do_refresh_token (PATH-shimmed `aws`) -----------
    //
    // These tests replace the `aws` binary on PATH with a small shell stub.
    // PATH is process-global, so the tests are serialised explicitly.

    #[cfg(unix)]
    mod refresh_token {
        use super::*;
        use serial_test::serial;
        use std::os::unix::fs::PermissionsExt;

        struct PathGuard {
            previous: Option<String>,
        }

        impl Drop for PathGuard {
            fn drop(&mut self) {
                match &self.previous {
                    Some(p) => std::env::set_var("PATH", p),
                    None => std::env::remove_var("PATH"),
                }
            }
        }

        /// Install a shell stub that pretends to be `aws`. Returns the guard
        /// (restores PATH on drop) and the tempdir (kept alive for the test).
        fn install_aws_stub(stub_body: &str) -> (PathGuard, tempfile::TempDir) {
            let tmp = tempfile::TempDir::new().unwrap();
            let stub = tmp.path().join("aws");
            fs::write(&stub, stub_body).unwrap();
            fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();

            let previous = std::env::var("PATH").ok();
            let new_path = match &previous {
                Some(p) => format!("{}:{}", tmp.path().display(), p),
                None => tmp.path().display().to_string(),
            };
            std::env::set_var("PATH", &new_path);

            (PathGuard { previous }, tmp)
        }

        /// Stub that always emits a successful create-token response.
        const STUB_OK: &str = r#"#!/bin/sh
cat <<'JSON'
{"accessToken":"new-access","expiresIn":28800,"refreshToken":"new-refresh"}
JSON
exit 0
"#;

        /// Stub that simulates the InvalidGrantException case: AWS rejected
        /// our refresh token. The cache will be checked afterwards to decide
        /// whether the session is alive.
        const STUB_INVALID_GRANT: &str = r#"#!/bin/sh
echo "An error occurred (InvalidGrantException)" >&2
exit 255
"#;

        /// Standard cache JSON pointing at acme. expiresAt and refreshToken
        /// are passed in so each test can vary them.
        fn cache_json(expires_at: &str, refresh_token: Option<&str>) -> String {
            let refresh = refresh_token
                .map(|t| format!(r#""refreshToken":"{t}","#))
                .unwrap_or_default();
            format!(
                r#"{{
                    "startUrl": "https://acme.awsapps.com/start",
                    "region": "ap-southeast-2",
                    "accessToken": "old-access",
                    "expiresAt": "{expires_at}",
                    "clientId": "abc",
                    "clientSecret": "shh",
                    "registrationExpiresAt": "2099-01-01T00:00:00Z",
                    {refresh}
                    "extraField": "ignored"
                }}"#,
                refresh = refresh
            )
        }

        fn profile() -> ProfileInfo {
            ProfileInfo {
                profile_name: "acme.dev".into(),
                sso_start_url: "https://acme.awsapps.com/start".into(),
                sso_region: "ap-southeast-2".into(),
                sso_account_id: "123456789012".into(),
                sso_role_name: "Admin".into(),
                session_name: "acme".into(),
                production: false,
            }
        }

        fn manager_with_cache_dir(cache_dir: &std::path::Path) -> AwsManager {
            AwsManager::new_with_paths(
                cache_dir.parent().unwrap().join("config"),
                cache_dir.to_path_buf(),
            )
        }

        // -- happy path -------------------------------------------------------

        #[test]
        #[serial]
        fn refresh_succeeds_writes_updated_cache() {
            let (_guard, _stub) = install_aws_stub(STUB_OK);

            let tmp = tempfile::TempDir::new().unwrap();
            let cache_dir = tmp.path().join("cache");
            fs::create_dir_all(&cache_dir).unwrap();
            let cache_file = cache_dir.join("entry.json");
            fs::write(
                &cache_file,
                cache_json("2026-01-01T00:00:00Z", Some("old-refresh")),
            )
            .unwrap();

            let mgr = manager_with_cache_dir(&cache_dir);
            assert!(mgr.try_refresh_token(&profile()));

            // Verify the cache was updated.
            let body = fs::read_to_string(&cache_file).unwrap();
            assert!(
                body.contains("new-access"),
                "access token should be updated"
            );
            assert!(
                body.contains("new-refresh"),
                "refresh token should rotate to the new one"
            );
            // Old token should be gone.
            assert!(!body.contains("old-access"));
        }

        #[test]
        #[serial]
        fn refresh_writes_atomically_no_tmp_leftovers() {
            // After a successful refresh the cache directory should contain
            // exactly the original entry, not a leftover .tmp file from the
            // tmp-and-rename strategy used by do_refresh_token.
            let (_guard, _stub) = install_aws_stub(STUB_OK);

            let tmp = tempfile::TempDir::new().unwrap();
            let cache_dir = tmp.path().join("cache");
            fs::create_dir_all(&cache_dir).unwrap();
            fs::write(
                cache_dir.join("entry.json"),
                cache_json("2026-01-01T00:00:00Z", Some("old-refresh")),
            )
            .unwrap();

            let mgr = manager_with_cache_dir(&cache_dir);
            assert!(mgr.try_refresh_token(&profile()));

            let entries: Vec<_> = fs::read_dir(&cache_dir)
                .unwrap()
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().into_string().unwrap())
                .collect();
            assert_eq!(
                entries.len(),
                1,
                "expected exactly one cache file, got {entries:?}"
            );
            assert_eq!(entries[0], "entry.json");
        }

        // -- token rotation by another process --------------------------------

        #[test]
        #[serial]
        fn refresh_recovers_when_other_process_already_rotated() {
            // Simulate: our refresh request fails because another tool
            // (AWS CLI / SDK / SOPS) already rotated the refresh token.
            // The cache on disk now holds a fresh, valid token written by
            // that other process. try_refresh_token should detect this and
            // return true rather than warning the user.
            let (_guard, _stub) = install_aws_stub(STUB_INVALID_GRANT);

            let tmp = tempfile::TempDir::new().unwrap();
            let cache_dir = tmp.path().join("cache");
            fs::create_dir_all(&cache_dir).unwrap();
            // expiresAt is in the future.
            fs::write(
                cache_dir.join("entry.json"),
                cache_json("2099-01-01T00:00:00Z", Some("rotated-by-other-tool")),
            )
            .unwrap();

            let mgr = manager_with_cache_dir(&cache_dir);
            assert!(
                mgr.try_refresh_token(&profile()),
                "session is alive even though our refresh failed"
            );
        }

        #[test]
        #[serial]
        fn refresh_fails_when_token_genuinely_dead() {
            // Refresh request fails AND the cache says the token is expired.
            // Nothing to recover; report failure honestly.
            let (_guard, _stub) = install_aws_stub(STUB_INVALID_GRANT);

            let tmp = tempfile::TempDir::new().unwrap();
            let cache_dir = tmp.path().join("cache");
            fs::create_dir_all(&cache_dir).unwrap();
            fs::write(
                cache_dir.join("entry.json"),
                cache_json("2020-01-01T00:00:00Z", Some("old-refresh")),
            )
            .unwrap();

            let mgr = manager_with_cache_dir(&cache_dir);
            assert!(!mgr.try_refresh_token(&profile()));
        }

        // -- early-return branches (no AWS call) ------------------------------

        #[test]
        #[serial]
        fn refresh_skips_when_no_refresh_token_in_cache() {
            // Stub would always succeed if called, but the cache has no
            // refresh token, so we shouldn't even try.
            let (_guard, _stub) = install_aws_stub(STUB_OK);

            let tmp = tempfile::TempDir::new().unwrap();
            let cache_dir = tmp.path().join("cache");
            fs::create_dir_all(&cache_dir).unwrap();
            fs::write(
                cache_dir.join("entry.json"),
                cache_json("2026-01-01T00:00:00Z", None),
            )
            .unwrap();

            let mgr = manager_with_cache_dir(&cache_dir);
            // Cache says expiresAt is in 2026 (past for our test fixture, but
            // try_refresh_token's fallback only treats it as "alive" when it's
            // in the future). 2026-01-01 may be in the past at runtime, so
            // expect false.
            assert!(!mgr.try_refresh_token(&profile()));
        }

        #[test]
        #[serial]
        fn refresh_skips_when_client_registration_expired() {
            // Stub would succeed, but registrationExpiresAt is in the past,
            // so do_refresh_token bails before invoking it.
            let (_guard, _stub) = install_aws_stub(STUB_OK);

            let tmp = tempfile::TempDir::new().unwrap();
            let cache_dir = tmp.path().join("cache");
            fs::create_dir_all(&cache_dir).unwrap();
            // Hand-build the cache with an expired registration timestamp.
            let body = r#"{
                "startUrl": "https://acme.awsapps.com/start",
                "region": "ap-southeast-2",
                "accessToken": "old",
                "expiresAt": "2020-01-01T00:00:00Z",
                "clientId": "abc",
                "clientSecret": "shh",
                "registrationExpiresAt": "2020-01-01T00:00:00Z",
                "refreshToken": "old-refresh"
            }"#;
            fs::write(cache_dir.join("entry.json"), body).unwrap();

            let mgr = manager_with_cache_dir(&cache_dir);
            assert!(!mgr.try_refresh_token(&profile()));
        }

        #[test]
        #[serial]
        fn refresh_skips_when_cache_file_missing() {
            let (_guard, _stub) = install_aws_stub(STUB_OK);

            let tmp = tempfile::TempDir::new().unwrap();
            let cache_dir = tmp.path().join("cache");
            fs::create_dir_all(&cache_dir).unwrap();
            // No cache file written.

            let mgr = manager_with_cache_dir(&cache_dir);
            assert!(!mgr.try_refresh_token(&profile()));
        }

        #[test]
        #[serial]
        fn refresh_skips_on_malformed_cache_json() {
            let (_guard, _stub) = install_aws_stub(STUB_OK);

            let tmp = tempfile::TempDir::new().unwrap();
            let cache_dir = tmp.path().join("cache");
            fs::create_dir_all(&cache_dir).unwrap();
            // File matches by extension but content is junk.
            fs::write(cache_dir.join("entry.json"), "not json").unwrap();

            let mgr = manager_with_cache_dir(&cache_dir);
            assert!(!mgr.try_refresh_token(&profile()));
        }
    }
}
