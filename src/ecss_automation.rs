//! ECSS Command Automation Framework
//!
//! This library provides a framework for automating ECSS-E-ST-70-41C telecommand execution
//! based on configuration files (TOML or JSON).
//!
//! # Features
//!
//! - Define command sequences in TOML or JSON configuration files
//! - Support for timing, delays, and conditional execution
//! - Batch processing of multiple commands
//! - Error handling and logging
//! - Extensible command registry
//!
//! # Configuration Format
//!
//! Commands can be defined in TOML:
//!
//! ```toml
//! [[commands]]
//! name = "power_on"
//! apid = 100
//! delay_ms = 1000
//! data = [0x01, 0x00, 0xFF]
//!
//! [[commands]]
//! name = "configure_antenna"
//! apid = 101
//! delay_ms = 2000
//! data = [0x02, 0x01]
//! ```
//!
//! Or in JSON:
//!
//! ```json
//! {
//!   "target": "192.168.1.100:5000",
//!   "commands": [
//!     {
//!       "name": "power_on",
//!       "apid": 100,
//!       "delay_ms": 1000,
//!       "data": [1, 0, 255]
//!     }
//!   ]
//! }
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};
use thiserror::Error;

use crate::ecss_udp::{EcssUdpClient, TelecommandPacket};

/// Errors that can occur during automation
#[derive(Error, Debug)]
pub enum AutomationError {
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("TOML parse error: {0}")]
    TomlError(#[from] toml::de::Error),

    #[error("JSON parse error: {0}")]
    JsonError(#[from] serde_json::Error),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Command execution error: {0}")]
    ExecutionError(String),

    #[error("Invalid file format: {0}")]
    InvalidFormat(String),

    #[error("Command not found: {0}")]
    CommandNotFound(String),

    #[error("Network error: {0}")]
    NetworkError(String),

    #[error("Validation error: {0}")]
    ValidationError(String),
}

/// Result type for automation operations
pub type AutomationResult<T> = Result<T, AutomationError>;

/// A single ECSS command definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandDefinition {
    /// Command name for identification
    pub name: String,

    /// Application Process ID (0-2047)
    pub apid: u16,

    /// Delay in milliseconds before sending this command
    #[serde(default)]
    pub delay_ms: u64,

    /// Command data payload
    pub data: Vec<u8>,

    /// Optional description
    #[serde(default)]
    pub description: String,

    /// Retry count on failure (0 = no retry)
    #[serde(default)]
    pub retry_count: u8,

    /// Optional metadata for custom handling
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

impl CommandDefinition {
    /// Creates a new command definition
    pub fn new(name: String, apid: u16, data: Vec<u8>) -> Self {
        CommandDefinition {
            name,
            apid,
            delay_ms: 0,
            data,
            description: String::new(),
            retry_count: 0,
            metadata: HashMap::new(),
        }
    }

    /// Sets the delay before executing this command
    pub fn with_delay(mut self, delay_ms: u64) -> Self {
        self.delay_ms = delay_ms;
        self
    }

    /// Sets the description
    pub fn with_description(mut self, description: String) -> Self {
        self.description = description;
        self
    }

    /// Sets retry count
    pub fn with_retry(mut self, retry_count: u8) -> Self {
        self.retry_count = retry_count;
        self
    }

    /// Validates the command
    pub fn validate(&self) -> AutomationResult<()> {
        if self.apid > 2047 {
            return Err(AutomationError::ValidationError(format!(
                "APID {} exceeds maximum of 2047",
                self.apid
            )));
        }

        if self.name.is_empty() {
            return Err(AutomationError::ValidationError(
                "Command name cannot be empty".to_string(),
            ));
        }

        Ok(())
    }
}

/// Configuration for ECSS command automation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationConfig {
    /// Target address (IP:Port)
    pub target: String,

    /// Local address to bind to
    #[serde(default = "default_local_addr")]
    pub local: String,

    /// List of commands to execute
    pub commands: Vec<CommandDefinition>,

    /// Global timeout in milliseconds (0 = no timeout)
    #[serde(default)]
    pub timeout_ms: u64,

    /// Stop on first error
    #[serde(default = "default_stop_on_error")]
    pub stop_on_error: bool,

    /// Repeat the entire sequence N times (0 = once)
    #[serde(default)]
    pub repeat_count: u8,
}

fn default_local_addr() -> String {
    "0.0.0.0:0".to_string()
}

fn default_stop_on_error() -> bool {
    true
}

impl AutomationConfig {
    /// Validates the entire configuration
    pub fn validate(&self) -> AutomationResult<()> {
        // Validate target address format
        self.target.parse::<std::net::SocketAddr>().map_err(|e| {
            AutomationError::ValidationError(format!("Invalid target address: {}", e))
        })?;

        if self.commands.is_empty() {
            return Err(AutomationError::ConfigError(
                "No commands defined in configuration".to_string(),
            ));
        }

        // Validate each command
        for cmd in &self.commands {
            cmd.validate()?;
        }

        Ok(())
    }

    /// Gets total estimated execution time in milliseconds
    pub fn estimated_duration_ms(&self) -> u64 {
        self.commands.iter().map(|c| c.delay_ms).sum()
    }

    /// Gets the number of commands
    pub fn command_count(&self) -> usize {
        self.commands.len()
    }
}

/// Execution statistics
#[derive(Debug, Clone)]
pub struct ExecutionStats {
    /// Total commands executed successfully
    pub successful: usize,

    /// Total commands failed
    pub failed: usize,

    /// Total time elapsed in milliseconds
    pub elapsed_ms: u64,

    /// Command execution times
    pub command_times: HashMap<String, u64>,
}

impl ExecutionStats {
    /// Creates a new stats tracker
    pub fn new() -> Self {
        ExecutionStats {
            successful: 0,
            failed: 0,
            elapsed_ms: 0,
            command_times: HashMap::new(),
        }
    }

    /// Gets success rate as percentage (0-100)
    pub fn success_rate(&self) -> f32 {
        let total = self.successful + self.failed;
        if total == 0 {
            0.0
        } else {
            (self.successful as f32 / total as f32) * 100.0
        }
    }
}

impl Default for ExecutionStats {
    fn default() -> Self {
        Self::new()
    }
}

/// ECSS Command Automation Engine
pub struct AutomationEngine {
    config: AutomationConfig,
    client: EcssUdpClient,
    sequence_counter: u16,
}

impl AutomationEngine {
    /// Creates a new automation engine from a configuration
    pub fn new(config: AutomationConfig) -> AutomationResult<Self> {
        config.validate()?;

        let client = EcssUdpClient::new(&config.local, &config.target).map_err(|e| {
            AutomationError::NetworkError(format!("Failed to create UDP client: {}", e))
        })?;

        Ok(AutomationEngine {
            config,
            client,
            sequence_counter: 0,
        })
    }

    /// Loads configuration from a TOML file
    pub fn from_toml_file<P: AsRef<Path>>(path: P) -> AutomationResult<Self> {
        let contents = fs::read_to_string(path)?;
        let config: AutomationConfig = toml::from_str(&contents)?;
        Self::new(config)
    }

    /// Loads configuration from a JSON file
    pub fn from_json_file<P: AsRef<Path>>(path: P) -> AutomationResult<Self> {
        let contents = fs::read_to_string(path)?;
        let config: AutomationConfig = serde_json::from_str(&contents)?;
        Self::new(config)
    }

    /// Loads configuration from a TOML string
    pub fn from_toml_str(toml_str: &str) -> AutomationResult<Self> {
        let config: AutomationConfig = toml::from_str(toml_str)?;
        Self::new(config)
    }

    /// Loads configuration from a JSON string
    pub fn from_json_str(json_str: &str) -> AutomationResult<Self> {
        let config: AutomationConfig = serde_json::from_str(json_str)?;
        Self::new(config)
    }

    /// Executes a single command
    fn execute_command(&mut self, cmd: &CommandDefinition) -> AutomationResult<()> {
        // Create the ECSS packet
        let packet =
            TelecommandPacket::new(cmd.apid, self.sequence_counter, cmd.data.clone(), false);

        // Send the packet
        self.client.send_command(&packet).map_err(|e| {
            AutomationError::ExecutionError(format!("Failed to send command '{}': {}", cmd.name, e))
        })?;

        // Increment sequence counter
        self.sequence_counter = (self.sequence_counter + 1) % 16384;

        Ok(())
    }

    /// Executes a single command with retries
    fn execute_with_retry(&mut self, cmd: &CommandDefinition) -> AutomationResult<()> {
        let mut last_error = None;

        for attempt in 0..=cmd.retry_count {
            match self.execute_command(cmd) {
                Ok(_) => return Ok(()),
                Err(e) => {
                    last_error = Some(e);
                    if attempt < cmd.retry_count {
                        log::warn!(
                            "Command '{}' attempt {} failed, retrying...",
                            cmd.name,
                            attempt + 1
                        );
                    }
                }
            }
        }

        Err(last_error.unwrap())
    }

    /// Executes the entire automation sequence
    pub fn execute(&mut self) -> AutomationResult<ExecutionStats> {
        let start = Instant::now();
        let mut stats = ExecutionStats::new();

        let repeat_count = if self.config.repeat_count == 0 {
            1
        } else {
            self.config.repeat_count as usize
        };

        for iteration in 1..=repeat_count {
            if self.config.repeat_count > 0 {
                log::info!("Starting iteration {} of {}", iteration, repeat_count);
            }

            for cmd in &self.config.commands.clone() {
                // Check timeout
                if self.config.timeout_ms > 0
                    && start.elapsed().as_millis() > self.config.timeout_ms as u128
                {
                    return Err(AutomationError::ExecutionError(
                        "Execution timeout exceeded".to_string(),
                    ));
                }

                // Apply delay
                if cmd.delay_ms > 0 {
                    std::thread::sleep(Duration::from_millis(cmd.delay_ms));
                }

                // Execute command
                let cmd_start = Instant::now();
                match self.execute_with_retry(cmd) {
                    Ok(_) => {
                        let elapsed = cmd_start.elapsed().as_millis() as u64;
                        stats.command_times.insert(cmd.name.clone(), elapsed);
                        stats.successful += 1;
                        log::info!(
                            "Successfully executed command '{}' in {}ms",
                            cmd.name,
                            elapsed
                        );
                    }
                    Err(e) => {
                        stats.failed += 1;
                        log::error!("Failed to execute command '{}': {}", cmd.name, e);

                        if self.config.stop_on_error {
                            stats.elapsed_ms = start.elapsed().as_millis() as u64;
                            return Err(e);
                        }
                    }
                }
            }
        }

        stats.elapsed_ms = start.elapsed().as_millis() as u64;
        Ok(stats)
    }

    /// Gets the current configuration
    pub fn config(&self) -> &AutomationConfig {
        &self.config
    }

    /// Updates the target address
    pub fn set_target(&mut self, target: &str) -> AutomationResult<()> {
        self.config.target = target.to_string();
        self.client = EcssUdpClient::new(&self.config.local, &self.config.target).map_err(|e| {
            AutomationError::NetworkError(format!("Failed to update target: {}", e))
        })?;
        Ok(())
    }

    /// Gets the current sequence counter
    pub fn sequence_counter(&self) -> u16 {
        self.sequence_counter
    }

    /// Resets the sequence counter
    pub fn reset_sequence_counter(&mut self) {
        self.sequence_counter = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_definition_creation() {
        let cmd = CommandDefinition::new("test".to_string(), 100, vec![0x01, 0x02]);
        assert_eq!(cmd.name, "test");
        assert_eq!(cmd.apid, 100);
        assert_eq!(cmd.data, vec![0x01, 0x02]);
    }

    #[test]
    fn test_command_definition_with_delay() {
        let cmd = CommandDefinition::new("test".to_string(), 100, vec![])
            .with_delay(1000)
            .with_description("Test command".to_string());

        assert_eq!(cmd.delay_ms, 1000);
        assert_eq!(cmd.description, "Test command");
    }

    #[test]
    fn test_command_validation_valid_apid() {
        let cmd = CommandDefinition::new("test".to_string(), 2047, vec![]);
        assert!(cmd.validate().is_ok());
    }

    #[test]
    fn test_command_validation_invalid_apid() {
        let cmd = CommandDefinition::new("test".to_string(), 2048, vec![]);
        assert!(cmd.validate().is_err());
    }

    #[test]
    fn test_command_validation_empty_name() {
        let cmd = CommandDefinition::new(String::new(), 100, vec![]);
        assert!(cmd.validate().is_err());
    }

    #[test]
    fn test_automation_config_toml_parse() {
        let toml_str = r#"
target = "127.0.0.1:5000"
[[commands]]
name = "test"
apid = 100
data = [1, 2, 3]
"#;
        let config: AutomationConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.target, "127.0.0.1:5000");
        assert_eq!(config.commands.len(), 1);
        assert_eq!(config.commands[0].name, "test");
    }

    #[test]
    fn test_automation_config_json_parse() {
        let json_str = r#"{
  "target": "127.0.0.1:5000",
  "commands": [
    {
      "name": "test",
      "apid": 100,
      "data": [1, 2, 3]
    }
  ]
}"#;
        let config: AutomationConfig = serde_json::from_str(json_str).unwrap();
        assert_eq!(config.target, "127.0.0.1:5000");
        assert_eq!(config.commands.len(), 1);
    }

    #[test]
    fn test_config_validation_valid() {
        let config = AutomationConfig {
            target: "127.0.0.1:5000".to_string(),
            local: default_local_addr(),
            commands: vec![CommandDefinition::new("test".to_string(), 100, vec![])],
            timeout_ms: 0,
            stop_on_error: true,
            repeat_count: 0,
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_config_validation_invalid_address() {
        let config = AutomationConfig {
            target: "invalid_address".to_string(),
            local: default_local_addr(),
            commands: vec![CommandDefinition::new("test".to_string(), 100, vec![])],
            timeout_ms: 0,
            stop_on_error: true,
            repeat_count: 0,
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validation_no_commands() {
        let config = AutomationConfig {
            target: "127.0.0.1:5000".to_string(),
            local: default_local_addr(),
            commands: vec![],
            timeout_ms: 0,
            stop_on_error: true,
            repeat_count: 0,
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_execution_stats_success_rate() {
        let mut stats = ExecutionStats::new();
        stats.successful = 8;
        stats.failed = 2;
        assert_eq!(stats.success_rate(), 80.0);
    }

    #[test]
    fn test_execution_stats_success_rate_empty() {
        let stats = ExecutionStats::new();
        assert_eq!(stats.success_rate(), 0.0);
    }

    #[test]
    fn test_execution_stats_success_rate_all_success() {
        let mut stats = ExecutionStats::new();
        stats.successful = 10;
        stats.failed = 0;
        assert_eq!(stats.success_rate(), 100.0);
    }

    #[test]
    fn test_config_estimated_duration() {
        let config = AutomationConfig {
            target: "127.0.0.1:5000".to_string(),
            local: default_local_addr(),
            commands: vec![
                CommandDefinition::new("test1".to_string(), 100, vec![]).with_delay(100),
                CommandDefinition::new("test2".to_string(), 101, vec![]).with_delay(200),
                CommandDefinition::new("test3".to_string(), 102, vec![]).with_delay(300),
            ],
            timeout_ms: 0,
            stop_on_error: true,
            repeat_count: 0,
        };
        assert_eq!(config.estimated_duration_ms(), 600);
    }

    #[test]
    fn test_config_command_count() {
        let config = AutomationConfig {
            target: "127.0.0.1:5000".to_string(),
            local: default_local_addr(),
            commands: vec![
                CommandDefinition::new("test1".to_string(), 100, vec![]),
                CommandDefinition::new("test2".to_string(), 101, vec![]),
            ],
            timeout_ms: 0,
            stop_on_error: true,
            repeat_count: 0,
        };
        assert_eq!(config.command_count(), 2);
    }
}
