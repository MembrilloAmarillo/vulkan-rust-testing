//! ECSS Command Automation Framework - Comprehensive Examples
//!
//! This example demonstrates the automation framework for executing ECSS commands
//! based on configuration files (TOML or JSON)

use rust_and_vulkan::ecss_automation::{AutomationConfig, AutomationEngine, CommandDefinition};

/// Example 1: Create and execute an automation from programmatic configuration
fn example_programmatic_config() {
    println!("\n╔══════════════════════════════════════════════════════════╗");
    println!("║  Example 1: Programmatic Configuration                 ║");
    println!("╚══════════════════════════════════════════════════════════╝\n");

    // Create a configuration programmatically
    let config = AutomationConfig {
        target: "127.0.0.1:5000".to_string(),
        local: "0.0.0.0:0".to_string(),
        commands: vec![
            CommandDefinition::new("initialize_system".to_string(), 100, vec![0x01, 0x00])
                .with_delay(100)
                .with_description("Initialize system".to_string())
                .with_retry(2),
            CommandDefinition::new("enable_antenna".to_string(), 101, vec![0x02, 0x01])
                .with_delay(500)
                .with_description("Enable antenna subsystem".to_string()),
            CommandDefinition::new("configure_power".to_string(), 100, vec![0x03, 0x50])
                .with_delay(200)
                .with_description("Configure power level to 50%".to_string()),
        ],
        timeout_ms: 30000,
        stop_on_error: false,
        repeat_count: 0,
    };

    // Display configuration information
    println!("Configuration Details:");
    println!("  Target: {}", config.target);
    println!("  Number of commands: {}", config.command_count());
    println!("  Estimated duration: {}ms", config.estimated_duration_ms());
    println!("  Stop on error: {}", config.stop_on_error);
    println!("  Timeout: {}ms", config.timeout_ms);
    println!();

    // List all commands
    println!("Commands:");
    for (i, cmd) in config.commands.iter().enumerate() {
        println!(
            "  {}. {} (APID: {}, Delay: {}ms)",
            i + 1,
            cmd.name,
            cmd.apid,
            cmd.delay_ms
        );
        if !cmd.description.is_empty() {
            println!("     Description: {}", cmd.description);
        }
        if cmd.retry_count > 0 {
            println!("     Retries: {}", cmd.retry_count);
        }
    }
    println!();

    // Validate configuration
    match config.validate() {
        Ok(_) => println!("✓ Configuration is valid\n"),
        Err(e) => println!("✗ Configuration error: {}\n", e),
    }
}

/// Example 2: Load configuration from TOML file
fn example_load_toml() {
    println!("╔══════════════════════════════════════════════════════════╗");
    println!("║  Example 2: Load TOML Configuration File                ║");
    println!("╚══════════════════════════════════════════════════════════╝\n");

    match AutomationEngine::from_toml_file("examples/ecss_automation_example.toml") {
        Ok(engine) => {
            let config = engine.config();
            println!("Successfully loaded TOML configuration!");
            println!();
            println!("Configuration Summary:");
            println!("  Target: {}", config.target);
            println!("  Commands: {}", config.command_count());
            println!("  Estimated duration: {}ms", config.estimated_duration_ms());
            println!("  Timeout: {}ms", config.timeout_ms);
            println!("  Stop on error: {}", config.stop_on_error);
            println!();

            println!("Command Details:");
            for cmd in &config.commands {
                println!("  • {} (APID: {})", cmd.name, cmd.apid);
                println!("    Description: {}", cmd.description);
                println!(
                    "    Delay: {}ms, Retries: {}",
                    cmd.delay_ms, cmd.retry_count
                );
                if !cmd.metadata.is_empty() {
                    for (k, v) in &cmd.metadata {
                        println!("    {}: {}", k, v);
                    }
                }
            }
            println!();
        }
        Err(e) => {
            println!("Note: Could not load TOML file (expected if file not found)");
            println!("Error: {}\n", e);
        }
    }
}

/// Example 3: Load configuration from JSON file
fn example_load_json() {
    println!("╔══════════════════════════════════════════════════════════╗");
    println!("║  Example 3: Load JSON Configuration File                ║");
    println!("╚══════════════════════════════════════════════════════════╝\n");

    match AutomationEngine::from_json_file("examples/ecss_automation_example.json") {
        Ok(engine) => {
            let config = engine.config();
            println!("Successfully loaded JSON configuration!");
            println!();
            println!("Configuration Summary:");
            println!("  Target: {}", config.target);
            println!("  Commands: {}", config.command_count());
            println!("  Estimated duration: {}ms", config.estimated_duration_ms());
            println!();
        }
        Err(e) => {
            println!("Note: Could not load JSON file (expected if file not found)");
            println!("Error: {}\n", e);
        }
    }
}

/// Example 4: Inline TOML configuration
fn example_inline_toml() {
    println!("╔══════════════════════════════════════════════════════════╗");
    println!("║  Example 4: Inline TOML Configuration                   ║");
    println!("╚══════════════════════════════════════════════════════════╝\n");

    let toml_config = r#"
target = "127.0.0.1:5000"
local = "0.0.0.0:0"
timeout_ms = 10000
stop_on_error = false
repeat_count = 0

[[commands]]
name = "power_on"
apid = 100
delay_ms = 0
data = [1, 0, 255]
description = "Power on the system"

[[commands]]
name = "configure_mode"
apid = 101
delay_ms = 500
data = [2, 3, 0]
description = "Set to operational mode"

[[commands]]
name = "enable_downlink"
apid = 102
delay_ms = 1000
data = [3, 1]
description = "Enable downlink communication"
"#;

    match AutomationEngine::from_toml_str(toml_config) {
        Ok(engine) => {
            let config = engine.config();
            println!("Successfully parsed inline TOML configuration!");
            println!();
            println!("Configuration:");
            println!("  Target: {}", config.target);
            println!("  Commands: {}", config.command_count());
            println!("  Total delay: {}ms", config.estimated_duration_ms());
            println!();

            for cmd in &config.commands {
                println!(
                    "  → {} (APID: {}, {}ms delay)",
                    cmd.name, cmd.apid, cmd.delay_ms
                );
                println!("    {}", cmd.description);
            }
            println!();
        }
        Err(e) => {
            println!("Error parsing TOML: {}\n", e);
        }
    }
}

/// Example 5: Inline JSON configuration
fn example_inline_json() {
    println!("╔══════════════════════════════════════════════════════════╗");
    println!("║  Example 5: Inline JSON Configuration                   ║");
    println!("╚══════════════════════════════════════════════════════════╝\n");

    let json_config = r#"{
  "target": "127.0.0.1:5000",
  "local": "0.0.0.0:0",
  "timeout_ms": 10000,
  "stop_on_error": false,
  "repeat_count": 0,
  "commands": [
    {
      "name": "init_payload",
      "apid": 200,
      "delay_ms": 0,
      "description": "Initialize payload",
      "data": [32, 1],
      "retry_count": 0,
      "metadata": {}
    },
    {
      "name": "start_measurement",
      "apid": 200,
      "delay_ms": 1000,
      "description": "Start measurement cycle",
      "data": [33, 255],
      "retry_count": 0,
      "metadata": {}
    }
  ]
}"#;

    match AutomationEngine::from_json_str(json_config) {
        Ok(engine) => {
            let config = engine.config();
            println!("Successfully parsed inline JSON configuration!");
            println!();
            println!("Configuration:");
            println!("  Target: {}", config.target);
            println!("  Commands: {}", config.command_count());
            println!("  Stop on error: {}", config.stop_on_error);
            println!();

            for cmd in &config.commands {
                println!("  → {} (APID: {})", cmd.name, cmd.apid);
                println!("    {}", cmd.description);
                println!("    Payload: {:02X?}", cmd.data);
            }
            println!();
        }
        Err(e) => {
            println!("Error parsing JSON: {}\n", e);
        }
    }
}

/// Example 6: Configuration validation
fn example_validation() {
    println!("╔══════════════════════════════════════════════════════════╗");
    println!("║  Example 6: Configuration Validation                    ║");
    println!("╚══════════════════════════════════════════════════════════╝\n");

    // Valid configuration
    let valid_config = AutomationConfig {
        target: "127.0.0.1:5000".to_string(),
        local: "0.0.0.0:0".to_string(),
        commands: vec![CommandDefinition::new("test".to_string(), 100, vec![])],
        timeout_ms: 0,
        stop_on_error: true,
        repeat_count: 0,
    };

    println!("Valid Configuration:");
    match valid_config.validate() {
        Ok(_) => println!("  ✓ Configuration is valid\n"),
        Err(e) => println!("  ✗ Error: {}\n", e),
    }

    // Invalid target address
    println!("Invalid Target Address:");
    let invalid_target = AutomationConfig {
        target: "not_an_address".to_string(),
        local: "0.0.0.0:0".to_string(),
        commands: vec![CommandDefinition::new("test".to_string(), 100, vec![])],
        timeout_ms: 0,
        stop_on_error: true,
        repeat_count: 0,
    };

    match invalid_target.validate() {
        Ok(_) => println!("  ✓ Valid\n"),
        Err(e) => println!("  ✗ Expected error: {}\n", e),
    }

    // Invalid APID
    println!("Invalid APID (> 2047):");
    let invalid_apid = CommandDefinition::new("test".to_string(), 2048, vec![]);
    match invalid_apid.validate() {
        Ok(_) => println!("  ✓ Valid\n"),
        Err(e) => println!("  ✗ Expected error: {}\n", e),
    }

    // Empty command list
    println!("Empty Command List:");
    let empty_commands = AutomationConfig {
        target: "127.0.0.1:5000".to_string(),
        local: "0.0.0.0:0".to_string(),
        commands: vec![],
        timeout_ms: 0,
        stop_on_error: true,
        repeat_count: 0,
    };

    match empty_commands.validate() {
        Ok(_) => println!("  ✓ Valid\n"),
        Err(e) => println!("  ✗ Expected error: {}\n", e),
    }
}

/// Example 7: Command definition with metadata
fn example_command_metadata() {
    println!("╔══════════════════════════════════════════════════════════╗");
    println!("║  Example 7: Command Metadata                            ║");
    println!("╚══════════════════════════════════════════════════════════╝\n");

    let mut cmd =
        CommandDefinition::new("complex_command".to_string(), 150, vec![0xAA, 0xBB, 0xCC])
            .with_delay(1500)
            .with_description("A complex command with metadata".to_string())
            .with_retry(3);

    cmd.metadata
        .insert("priority".to_string(), "high".to_string());
    cmd.metadata
        .insert("subsystem".to_string(), "Science".to_string());
    cmd.metadata
        .insert("operator".to_string(), "mission_control".to_string());

    println!("Command Details:");
    println!("  Name: {}", cmd.name);
    println!("  APID: {}", cmd.apid);
    println!("  Delay: {}ms", cmd.delay_ms);
    println!("  Retries: {}", cmd.retry_count);
    println!("  Description: {}", cmd.description);
    println!("  Data: {:02X?}", cmd.data);
    println!();

    println!("Metadata:");
    for (key, value) in &cmd.metadata {
        println!("  {}: {}", key, value);
    }
    println!();
}

fn main() {
    println!("\n╔══════════════════════════════════════════════════════════╗");
    println!("║  ECSS Command Automation Framework - Examples           ║");
    println!("║  European Cooperation for Space Standardization         ║");
    println!("╚══════════════════════════════════════════════════════════╝");

    // Run all examples
    example_programmatic_config();
    example_load_toml();
    example_load_json();
    example_inline_toml();
    example_inline_json();
    example_validation();
    example_command_metadata();

    println!("╔══════════════════════════════════════════════════════════╗");
    println!("║  All examples completed successfully!                   ║");
    println!("╚══════════════════════════════════════════════════════════╝\n");
}
