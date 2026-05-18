use serde_json;
use std::fs;
use std::process::Command;
use tempfile::TempDir;

// Import the types we need to test
use kee::{KeeConfig, ProfileInfo};

#[cfg(test)]
mod integration_tests {
    use super::*;

    #[test]
    fn test_binary_help_command() {
        let output = Command::new("cargo")
            .args(&["run", "--", "--help"])
            .output()
            .expect("Failed to execute command");

        assert!(output.status.success());
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(stdout.contains("AWS CLI profile manager"));
        assert!(stdout.contains("Commands:"));
        assert!(stdout.contains("add"));
        assert!(stdout.contains("use"));
        assert!(stdout.contains("ls"));
        assert!(stdout.contains("current"));
        assert!(stdout.contains("rm"));
    }

    #[test]
    fn test_binary_version_command() {
        let output = Command::new("cargo")
            .args(&["run", "--", "--version"])
            .output()
            .expect("Failed to execute version command");

        assert!(output.status.success());
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(stdout.contains("kee"));
        assert!(stdout.contains(env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn test_list_command_no_profiles() {
        let temp_dir = TempDir::new().unwrap();

        let output = Command::new("cargo")
            .args(&["run", "--", "ls"])
            .env("HOME", temp_dir.path())
            .output()
            .expect("Failed to execute list command");

        assert!(output.status.success());
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(stdout.contains("No profiles configured"));
    }

    #[test]
    fn test_list_command_names_flag() {
        let temp_dir = TempDir::new().unwrap();

        let output = Command::new("cargo")
            .args(&["run", "--", "ls", "--names"])
            .env("HOME", temp_dir.path())
            .output()
            .expect("Failed to execute list command with names flag");

        assert!(output.status.success());
        let stdout = String::from_utf8(output.stdout).unwrap();
        // With --names on empty config, output should be clean (no help text)
        assert!(stdout.trim().is_empty());
    }

    #[test]
    fn test_current_command_no_session() {
        let temp_dir = TempDir::new().unwrap();

        let output = Command::new("cargo")
            .args(&["run", "--", "current"])
            .env("HOME", temp_dir.path())
            .output()
            .expect("Failed to execute current command");

        assert!(output.status.success());
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(stdout.contains("No profile is currently active"));
    }

    #[test]
    fn test_invalid_command() {
        let output = Command::new("cargo")
            .args(&["run", "--", "invalid-command"])
            .output()
            .expect("Failed to execute invalid command");

        assert!(!output.status.success());
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(stderr.contains("error") || stderr.contains("unrecognized"));
    }

    #[test]
    fn test_remove_nonexistent_profile() {
        let temp_dir = TempDir::new().unwrap();

        let output = Command::new("cargo")
            .args(&["run", "--", "rm", "nonexistent-profile"])
            .env("HOME", temp_dir.path())
            .stdin(std::process::Stdio::piped())
            .output()
            .expect("Failed to execute remove command");

        // Command should succeed but show profile not found
        assert!(output.status.success());
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(stdout.contains("not found"));
    }

    #[test]
    fn test_use_nonexistent_profile() {
        let temp_dir = TempDir::new().unwrap();

        let output = Command::new("cargo")
            .args(&["run", "--", "use", "nonexistent-profile"])
            .env("HOME", temp_dir.path())
            .stdin(std::process::Stdio::piped())
            .output()
            .expect("Failed to execute use command");

        // Command should succeed but show profile not found
        assert!(output.status.success());
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(stdout.contains("not found"));
    }
}

#[cfg(test)]
mod aws_config_tests {
    use configparser::ini::Ini;

    #[test]
    fn test_aws_config_parsing() {
        let aws_config_content = r#"
[profile test-profile]
sso_start_url = https://test.awsapps.com/start
sso_region = us-east-1
sso_account_id = 123456789012
sso_role_name = TestRole
sso_session = test-session

[sso-session test-session]
sso_start_url = https://test.awsapps.com/start
sso_region = us-east-1

[profile other-profile]
sso_account_id = 999999999999
"#;

        let mut config = Ini::new();
        config.read(aws_config_content.to_string()).unwrap();

        let section = config.get_map_ref().get("profile test-profile").unwrap();

        assert_eq!(
            section.get("sso_start_url").unwrap().as_ref().unwrap(),
            "https://test.awsapps.com/start"
        );
        assert_eq!(
            section.get("sso_region").unwrap().as_ref().unwrap(),
            "us-east-1"
        );
        assert_eq!(
            section.get("sso_account_id").unwrap().as_ref().unwrap(),
            "123456789012"
        );
        assert_eq!(
            section.get("sso_role_name").unwrap().as_ref().unwrap(),
            "TestRole"
        );
        assert_eq!(
            section.get("sso_session").unwrap().as_ref().unwrap(),
            "test-session"
        );
    }

    #[test]
    fn test_aws_config_with_sso_session() {
        let aws_config_content = r#"
[profile modern-profile]
sso_session = company-session
sso_account_id = 123456789012
sso_role_name = ModernRole

[sso-session company-session]
sso_start_url = https://company.awsapps.com/start
sso_region = us-west-2
"#;

        let mut config = Ini::new();
        config.read(aws_config_content.to_string()).unwrap();

        // Test profile section
        let profile_section = config.get_map_ref().get("profile modern-profile").unwrap();
        assert_eq!(
            profile_section
                .get("sso_session")
                .unwrap()
                .as_ref()
                .unwrap(),
            "company-session"
        );
        assert_eq!(
            profile_section
                .get("sso_account_id")
                .unwrap()
                .as_ref()
                .unwrap(),
            "123456789012"
        );

        // Test sso-session section
        let sso_section = config
            .get_map_ref()
            .get("sso-session company-session")
            .unwrap();
        assert_eq!(
            sso_section.get("sso_start_url").unwrap().as_ref().unwrap(),
            "https://company.awsapps.com/start"
        );
        assert_eq!(
            sso_section.get("sso_region").unwrap().as_ref().unwrap(),
            "us-west-2"
        );
    }
}

#[cfg(test)]
mod config_file_tests {
    use super::*;

    #[test]
    fn test_config_roundtrip_with_file() {
        let temp_dir = TempDir::new().unwrap();
        let config_file = temp_dir.path().join("config.json");

        // Create original config
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

        original_config
            .profiles
            .insert("test".to_string(), profile.clone());
        original_config.current_profile = Some("test".to_string());

        // Save to file
        let json = serde_json::to_string_pretty(&original_config).unwrap();
        fs::write(&config_file, json).unwrap();

        // Load from file
        let loaded_json = fs::read_to_string(&config_file).unwrap();
        let loaded_config: KeeConfig = serde_json::from_str(&loaded_json).unwrap();

        // Verify they match
        assert_eq!(original_config.profiles, loaded_config.profiles);
        assert_eq!(
            original_config.current_profile,
            loaded_config.current_profile
        );
        assert_eq!(loaded_config.profiles.get("test"), Some(&profile));
    }

    #[test]
    fn test_multiple_profiles_in_config() {
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

        // Add profiles
        config.profiles.insert("prod".to_string(), profile1.clone());
        config.profiles.insert("dev".to_string(), profile2.clone());

        // Test serialization with multiple profiles
        let json = serde_json::to_string_pretty(&config).unwrap();
        let deserialized: KeeConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.profiles.len(), 2);
        assert_eq!(deserialized.profiles.get("prod"), Some(&profile1));
        assert_eq!(deserialized.profiles.get("dev"), Some(&profile2));
    }
}
