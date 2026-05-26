use std::fs;
use tempfile::TempDir;

// Import the types we need to test
use kee::{KeeConfig, ProfileInfo};

#[cfg(test)]
mod config_tests {
    use super::*;

    #[test]
    fn test_kee_config_new() {
        let config = KeeConfig::default();
        assert!(config.profiles.is_empty());
        assert!(config.current_profile.is_none());
    }

    #[test]
    fn test_profile_info_creation() {
        let profile = ProfileInfo {
            profile_name: "test-profile".to_string(),
            sso_start_url: "https://test.awsapps.com/start".to_string(),
            sso_region: "us-east-1".to_string(),
            sso_account_id: "123456789012".to_string(),
            sso_role_name: "TestRole".to_string(),
            session_name: "test-session".to_string(),
            production: false,
        };

        assert_eq!(profile.profile_name, "test-profile");
        assert_eq!(profile.sso_account_id, "123456789012");
        assert_eq!(profile.sso_role_name, "TestRole");
    }

    #[test]
    fn test_config_serialization() {
        let mut config = KeeConfig::default();
        let profile = ProfileInfo {
            profile_name: "test-profile".to_string(),
            sso_start_url: "https://test.awsapps.com/start".to_string(),
            sso_region: "us-east-1".to_string(),
            sso_account_id: "123456789012".to_string(),
            sso_role_name: "TestRole".to_string(),
            session_name: "test-session".to_string(),
            production: false,
        };

        config.profiles.insert("test".to_string(), profile.clone());
        config.current_profile = Some("test".to_string());

        // Test serialization
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("test-profile"));
        assert!(json.contains("123456789012"));

        // Test deserialization
        let deserialized: KeeConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.profiles.len(), 1);
        assert_eq!(deserialized.current_profile, Some("test".to_string()));
        assert_eq!(deserialized.profiles.get("test"), Some(&profile));
    }

    #[test]
    fn test_config_with_multiple_profiles() {
        let mut config = KeeConfig::default();

        let profile1 = ProfileInfo {
            profile_name: "prod-profile".to_string(),
            sso_start_url: "https://prod.awsapps.com/start".to_string(),
            sso_region: "us-east-1".to_string(),
            sso_account_id: "111111111111".to_string(),
            sso_role_name: "ProdRole".to_string(),
            session_name: "prod-session".to_string(),
            production: false,
        };

        let profile2 = ProfileInfo {
            profile_name: "dev-profile".to_string(),
            sso_start_url: "https://dev.awsapps.com/start".to_string(),
            sso_region: "us-west-2".to_string(),
            sso_account_id: "222222222222".to_string(),
            sso_role_name: "DevRole".to_string(),
            session_name: "dev-session".to_string(),
            production: false,
        };

        config.profiles.insert("prod".to_string(), profile1.clone());
        config.profiles.insert("dev".to_string(), profile2.clone());

        assert_eq!(config.profiles.len(), 2);
        assert_eq!(config.profiles.get("prod"), Some(&profile1));
        assert_eq!(config.profiles.get("dev"), Some(&profile2));
    }

    #[test]
    fn test_empty_config_handling() {
        let config = KeeConfig::default();
        assert!(config.profiles.is_empty());
        assert!(config.current_profile.is_none());

        // Test serialization of empty config
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: KeeConfig = serde_json::from_str(&json).unwrap();
        assert!(deserialized.profiles.is_empty());
        assert!(deserialized.current_profile.is_none());
    }
}

#[cfg(test)]
mod file_operations_tests {
    use super::*;

    #[test]
    fn test_config_file_save_and_load() {
        let temp_dir = TempDir::new().unwrap();
        let config_file = temp_dir.path().join("config.json");

        let mut original_config = KeeConfig::default();
        let profile = ProfileInfo {
            profile_name: "test-profile".to_string(),
            sso_start_url: "https://test.awsapps.com/start".to_string(),
            sso_region: "us-east-1".to_string(),
            sso_account_id: "123456789012".to_string(),
            sso_role_name: "TestRole".to_string(),
            session_name: "test-session".to_string(),
            production: false,
        };

        original_config.profiles.insert("test".to_string(), profile);
        original_config.current_profile = Some("test".to_string());

        // Save config
        let json = serde_json::to_string_pretty(&original_config).unwrap();
        fs::write(&config_file, json).unwrap();

        // Load config
        let loaded_json = fs::read_to_string(&config_file).unwrap();
        let loaded_config: KeeConfig = serde_json::from_str(&loaded_json).unwrap();

        // Verify they match
        assert_eq!(original_config.profiles, loaded_config.profiles);
        assert_eq!(
            original_config.current_profile,
            loaded_config.current_profile
        );
    }

    #[test]
    fn test_malformed_config_handling() {
        let temp_dir = TempDir::new().unwrap();
        let config_file = temp_dir.path().join("config.json");

        // Write malformed JSON
        fs::write(&config_file, "{ invalid json }").unwrap();

        // Should handle gracefully when loading
        let content = fs::read_to_string(&config_file).unwrap();
        let result: Result<KeeConfig, _> = serde_json::from_str(&content);
        assert!(result.is_err());
    }
}

#[cfg(test)]
mod utility_tests {
    use super::*;

    #[test]
    fn test_profile_info_clone() {
        let profile = ProfileInfo {
            profile_name: "test".to_string(),
            sso_start_url: "https://test.com".to_string(),
            sso_region: "us-east-1".to_string(),
            sso_account_id: "123456789012".to_string(),
            sso_role_name: "TestRole".to_string(),
            session_name: "session".to_string(),
            production: false,
        };

        let cloned = profile.clone();
        assert_eq!(profile.profile_name, cloned.profile_name);
        assert_eq!(profile.sso_account_id, cloned.sso_account_id);
    }

    #[test]
    fn test_version_from_cargo() {
        // Test that version is available from Cargo.toml
        let version = env!("CARGO_PKG_VERSION");
        assert!(!version.is_empty());
        assert!(version.contains('.'));

        // Should match semantic versioning pattern (basic check)
        let parts: Vec<&str> = version.split('.').collect();
        assert!(parts.len() >= 2); // At least major.minor

        // Each part should be numeric
        for part in parts {
            assert!(part.chars().all(|c| c.is_ascii_digit()));
        }
    }
}
