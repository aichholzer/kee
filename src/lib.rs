// Library module for testable components
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

mod aws;
pub use aws::ProfileInfo;

#[derive(Serialize, Deserialize, Debug, PartialEq, Default)]
pub struct KeeConfig {
    pub profiles: HashMap<String, ProfileInfo>,
    pub current_profile: Option<String>,
}

impl KeeConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_profile(&mut self, name: String, info: ProfileInfo) {
        self.profiles.insert(name, info);
    }

    pub fn remove_profile(&mut self, name: &str) -> Option<ProfileInfo> {
        let removed = self.profiles.remove(name);
        if self.current_profile.as_deref() == Some(name) {
            self.current_profile = None;
        }
        removed
    }

    pub fn get_profile(&self, name: &str) -> Option<&ProfileInfo> {
        self.profiles.get(name)
    }

    pub fn list_profiles(&self) -> Vec<(&String, &ProfileInfo)> {
        self.profiles.iter().collect()
    }

    pub fn set_current_profile(&mut self, name: Option<String>) {
        self.current_profile = name;
    }

    pub fn is_empty(&self) -> bool {
        self.profiles.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_profile_info_serialization() {
        let profile = ProfileInfo {
            profile_name: "kee-test".to_string(),
            sso_start_url: "https://test.awsapps.com/start".to_string(),
            sso_region: "us-east-1".to_string(),
            sso_account_id: "123456789012".to_string(),
            sso_role_name: "TestRole".to_string(),
            session_name: "test-session".to_string(),
            production: false,
        };

        let json = serde_json::to_string(&profile).unwrap();
        let deserialized: ProfileInfo = serde_json::from_str(&json).unwrap();

        assert_eq!(profile, deserialized);
    }

    #[test]
    fn test_kee_config_default() {
        let config = KeeConfig::default();
        assert!(config.profiles.is_empty());
        assert!(config.current_profile.is_none());
        assert!(config.is_empty());
    }

    #[test]
    fn test_kee_config_new() {
        let config = KeeConfig::new();
        assert!(config.profiles.is_empty());
        assert!(config.current_profile.is_none());
    }

    #[test]
    fn test_kee_config_add_profile() {
        let mut config = KeeConfig::new();
        let profile = ProfileInfo {
            profile_name: "kee-test".to_string(),
            sso_start_url: "https://test.awsapps.com/start".to_string(),
            sso_region: "us-east-1".to_string(),
            sso_account_id: "123456789012".to_string(),
            sso_role_name: "TestRole".to_string(),
            session_name: "test-session".to_string(),
            production: false,
        };

        config.add_profile("test".to_string(), profile.clone());

        assert!(!config.is_empty());
        assert_eq!(config.profiles.len(), 1);
        assert_eq!(config.get_profile("test"), Some(&profile));
    }

    #[test]
    fn test_kee_config_remove_profile() {
        let mut config = KeeConfig::new();
        let profile = ProfileInfo {
            profile_name: "kee-test".to_string(),
            sso_start_url: "https://test.awsapps.com/start".to_string(),
            sso_region: "us-east-1".to_string(),
            sso_account_id: "123456789012".to_string(),
            sso_role_name: "TestRole".to_string(),
            session_name: "test-session".to_string(),
            production: false,
        };

        config.add_profile("test".to_string(), profile.clone());
        config.set_current_profile(Some("test".to_string()));

        let removed = config.remove_profile("test");

        assert_eq!(removed, Some(profile));
        assert!(config.is_empty());
        assert!(config.current_profile.is_none());
    }

    #[test]
    fn test_kee_config_remove_nonexistent_profile() {
        let mut config = KeeConfig::new();
        let removed = config.remove_profile("nonexistent");
        assert!(removed.is_none());
    }

    #[test]
    fn test_kee_config_get_profile() {
        let mut config = KeeConfig::new();
        let profile = ProfileInfo {
            profile_name: "kee-test".to_string(),
            sso_start_url: "https://test.awsapps.com/start".to_string(),
            sso_region: "us-east-1".to_string(),
            sso_account_id: "123456789012".to_string(),
            sso_role_name: "TestRole".to_string(),
            session_name: "test-session".to_string(),
            production: false,
        };

        config.add_profile("test".to_string(), profile.clone());

        assert_eq!(config.get_profile("test"), Some(&profile));
        assert_eq!(config.get_profile("nonexistent"), None);
    }

    #[test]
    fn test_kee_config_list_profiles() {
        let mut config = KeeConfig::new();
        let profile1 = ProfileInfo {
            profile_name: "kee-test1".to_string(),
            sso_start_url: "https://test.awsapps.com/start".to_string(),
            sso_region: "us-east-1".to_string(),
            sso_account_id: "123456789012".to_string(),
            sso_role_name: "TestRole".to_string(),
            session_name: "test-session1".to_string(),
            production: false,
        };
        let profile2 = ProfileInfo {
            profile_name: "kee-test2".to_string(),
            sso_start_url: "https://test.awsapps.com/start".to_string(),
            sso_region: "us-west-2".to_string(),
            sso_account_id: "123456789013".to_string(),
            sso_role_name: "TestRole2".to_string(),
            session_name: "test-session2".to_string(),
            production: false,
        };

        config.add_profile("test1".to_string(), profile1);
        config.add_profile("test2".to_string(), profile2);

        let profiles = config.list_profiles();
        assert_eq!(profiles.len(), 2);

        let profile_names: Vec<&String> = profiles.iter().map(|(name, _)| *name).collect();
        assert!(profile_names.contains(&&"test1".to_string()));
        assert!(profile_names.contains(&&"test2".to_string()));
    }

    #[test]
    fn test_kee_config_set_current_profile() {
        let mut config = KeeConfig::new();

        config.set_current_profile(Some("test".to_string()));
        assert_eq!(config.current_profile, Some("test".to_string()));

        config.set_current_profile(None);
        assert!(config.current_profile.is_none());
    }

    #[test]
    fn test_kee_config_serialization() {
        let mut config = KeeConfig::new();
        let profile = ProfileInfo {
            profile_name: "kee-test".to_string(),
            sso_start_url: "https://test.awsapps.com/start".to_string(),
            sso_region: "us-east-1".to_string(),
            sso_account_id: "123456789012".to_string(),
            sso_role_name: "TestRole".to_string(),
            session_name: "test-session".to_string(),
            production: false,
        };

        config.add_profile("test".to_string(), profile);
        config.set_current_profile(Some("test".to_string()));

        let json = serde_json::to_string_pretty(&config).unwrap();
        let deserialized: KeeConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(config, deserialized);
    }
}
