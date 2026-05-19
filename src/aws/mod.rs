use chrono::Utc;
use configparser::ini::Ini;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::Command;

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
        let home_dir = dirs::home_dir().ok_or_else(|| {
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
    /// Returns true if the token was successfully refreshed, false otherwise.
    pub fn try_refresh_token(&self, profile_info: &ProfileInfo) -> bool {
        self.do_refresh_token(profile_info).is_some()
    }

    fn do_refresh_token(&self, profile_info: &ProfileInfo) -> Option<()> {
        let cache_file = self.find_sso_cache_file(profile_info)?;
        let content = fs::read_to_string(&cache_file).ok()?;
        let cache: SsoTokenCache = serde_json::from_str(&content).ok()?;

        let refresh_token = cache.refresh_token.as_ref().filter(|t| !t.is_empty())?;
        let client_id = cache.client_id.as_ref().filter(|id| !id.is_empty())?;
        let client_secret = cache.client_secret.as_ref().filter(|s| !s.is_empty())?;

        // Check that the client registration itself hasn't expired
        if let Some(ref reg_expires) = cache.registration_expires_at {
            let expires = reg_expires.parse::<chrono::DateTime<Utc>>().ok()?;
            if Utc::now() >= expires {
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
            .ok()
            .filter(|o| o.status.success())?;

        let response: CreateTokenResponse = serde_json::from_slice(&output.stdout).ok()?;

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
