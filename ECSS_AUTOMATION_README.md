# ECSS Command Automation Framework

A powerful Rust library for automating the execution of ECSS-E-ST-70-41C telecommand sequences defined in configuration files (TOML or JSON).

## Overview

The ECSS Command Automation Framework simplifies the creation and execution of complex command sequences for space systems. Define your commands once in a configuration file and execute them with a single function call.

## Features

- **Configuration-Driven**: Define command sequences in TOML or JSON
- **Flexible Timing**: Support for delays, timeouts, and retry logic
- **Error Handling**: Configurable error handling and validation
- **Metadata Support**: Attach custom metadata to commands
- **Programmatic API**: Create configurations dynamically in code
- **Comprehensive Validation**: Validate all parameters before execution
- **Execution Statistics**: Track success rates and timing information
- **Multiple Load Methods**: Load from files, strings, or programmatic configuration

## Installation

The module is already integrated in your Rust project:

```rust
use rust_and_vulkan::ecss_automation::{AutomationEngine, AutomationConfig, CommandDefinition};
```

## Configuration Format

### TOML Format

```toml
target = "192.168.1.100:5000"
local = "0.0.0.0:0"
timeout_ms = 60000
stop_on_error = true
repeat_count = 0

[[commands]]
name = "power_on"
apid = 100
delay_ms = 0
description = "Enable main power"
data = [0x01, 0x00, 0xFF]
retry_count = 2

[commands.metadata]
subsystem = "Power Management"
priority = "critical"

[[commands]]
name = "enable_antenna"
apid = 101
delay_ms = 1000
description = "Enable antenna subsystem"
data = [0x02, 0x01]
retry_count = 1
```

### JSON Format

```json
{
  "target": "192.168.1.100:5000",
  "local": "0.0.0.0:0",
  "timeout_ms": 60000,
  "stop_on_error": true,
  "repeat_count": 0,
  "commands": [
    {
      "name": "power_on",
      "apid": 100,
      "delay_ms": 0,
      "description": "Enable main power",
      "data": [1, 0, 255],
      "retry_count": 2,
      "metadata": {
        "subsystem": "Power Management",
        "priority": "critical"
      }
    },
    {
      "name": "enable_antenna",
      "apid": 101,
      "delay_ms": 1000,
      "description": "Enable antenna subsystem",
      "data": [2, 1],
      "retry_count": 1,
      "metadata": {}
    }
  ]
}
```

## Configuration Parameters

### Top-Level Configuration

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `target` | string | Required | Receiver IP and port (e.g., "192.168.1.100:5000") |
| `local` | string | "0.0.0.0:0" | Local address to bind to |
| `timeout_ms` | u64 | 0 | Global timeout in milliseconds (0 = no timeout) |
| `stop_on_error` | bool | true | Stop execution on first error |
| `repeat_count` | u8 | 0 | Repeat sequence N times (0 = execute once) |
| `commands` | array | Required | List of commands to execute |

### Command Parameters

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `name` | string | Required | Command identifier |
| `apid` | u16 | Required | APID (0-2047) |
| `data` | array | Required | Command payload bytes |
| `delay_ms` | u64 | 0 | Delay before executing |
| `description` | string | "" | Command description |
| `retry_count` | u8 | 0 | Retry attempts on failure |
| `metadata` | object | {} | Custom metadata (optional) |

## Usage Examples

### Load from TOML File

```rust
use rust_and_vulkan::ecss_automation::AutomationEngine;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut engine = AutomationEngine::from_toml_file("config.toml")?;
    let stats = engine.execute()?;
    
    println!("Executed: {}", stats.successful);
    println!("Failed: {}", stats.failed);
    println!("Success rate: {:.1}%", stats.success_rate());
    
    Ok(())
}
```

### Load from JSON File

```rust
let mut engine = AutomationEngine::from_json_file("config.json")?;
let stats = engine.execute()?;
```

### Load from String

```rust
let toml_str = r#"
target = "192.168.1.100:5000"
[[commands]]
name = "power_on"
apid = 100
data = [1, 0, 255]
"#;

let mut engine = AutomationEngine::from_toml_str(toml_str)?;
```

### Programmatic Configuration

```rust
use rust_and_vulkan::ecss_automation::{AutomationConfig, CommandDefinition};

let config = AutomationConfig {
    target: "192.168.1.100:5000".to_string(),
    local: "0.0.0.0:0".to_string(),
    commands: vec![
        CommandDefinition::new(
            "power_on".to_string(),
            100,
            vec![0x01, 0x00, 0xFF],
        ).with_delay(100).with_retry(2),
        
        CommandDefinition::new(
            "antenna".to_string(),
            101,
            vec![0x02, 0x01],
        ).with_delay(1000),
    ],
    timeout_ms: 60000,
    stop_on_error: true,
    repeat_count: 0,
};

let mut engine = AutomationEngine::new(config)?;
let stats = engine.execute()?;
```

## API Reference

### AutomationEngine

The main automation execution engine.

**Methods:**

- `new(config: AutomationConfig) -> Result<Self>` - Create engine from config
- `from_toml_file(path: &Path) -> Result<Self>` - Load from TOML file
- `from_json_file(path: &Path) -> Result<Self>` - Load from JSON file
- `from_toml_str(s: &str) -> Result<Self>` - Parse TOML string
- `from_json_str(s: &str) -> Result<Self>` - Parse JSON string
- `execute(&mut self) -> Result<ExecutionStats>` - Execute the sequence
- `config(&self) -> &AutomationConfig` - Get current configuration
- `set_target(&mut self, target: &str) -> Result<()>` - Change target address
- `sequence_counter(&self) -> u16` - Get current sequence counter
- `reset_sequence_counter(&mut self)` - Reset sequence counter

### CommandDefinition

Individual command specification.

**Fields:**

- `name: String` - Command name
- `apid: u16` - Application Process ID
- `delay_ms: u64` - Delay before execution
- `data: Vec<u8>` - Command payload
- `description: String` - Human-readable description
- `retry_count: u8` - Retry attempts
- `metadata: HashMap<String, String>` - Custom metadata

**Methods:**

- `new(name, apid, data) -> Self` - Create new command
- `with_delay(ms) -> Self` - Set delay
- `with_description(desc) -> Self` - Set description
- `with_retry(count) -> Self` - Set retry count
- `validate() -> Result<()>` - Validate command

### AutomationConfig

Configuration container for a complete automation sequence.

**Methods:**

- `validate() -> Result<()>` - Validate entire configuration
- `estimated_duration_ms() -> u64` - Get total delay time
- `command_count() -> usize` - Get number of commands

### ExecutionStats

Statistics from a completed execution.

**Fields:**

- `successful: usize` - Number of successful commands
- `failed: usize` - Number of failed commands
- `elapsed_ms: u64` - Total execution time
- `command_times: HashMap<String, u64>` - Individual command timings

**Methods:**

- `success_rate() -> f32` - Get success percentage (0-100)

## Error Handling

The framework uses `thiserror` for comprehensive error handling:

```rust
match engine.execute() {
    Ok(stats) => {
        println!("Success rate: {:.1}%", stats.success_rate());
    }
    Err(AutomationError::ExecutionError(msg)) => {
        eprintln!("Execution failed: {}", msg);
    }
    Err(AutomationError::ValidationError(msg)) => {
        eprintln!("Validation error: {}", msg);
    }
    Err(e) => {
        eprintln!("Error: {}", e);
    }
}
```

## Advanced Features

### Command Retry

Automatically retry failed commands:

```toml
[[commands]]
name = "critical_command"
apid = 100
data = [0x01, 0x02]
retry_count = 3  # Retry up to 3 times
```

### Command Metadata

Attach custom metadata to commands for external processing:

```toml
[[commands]]
name = "power_on"
apid = 100
data = [0x01, 0x00, 0xFF]

[commands.metadata]
priority = "critical"
subsystem = "Power Management"
operator = "mission_control"
```

### Global Timeout

Set a maximum execution time:

```toml
timeout_ms = 60000  # 60 second limit
stop_on_error = true
```

### Sequence Repetition

Execute the entire sequence multiple times:

```toml
repeat_count = 3  # Run the sequence 3 times
```

### Dynamic Target Updates

Change the target address at runtime:

```rust
let mut engine = AutomationEngine::from_toml_file("config.toml")?;
engine.set_target("192.168.1.101:5000")?;  // Change target
engine.execute()?;
```

## Validation

The framework validates:

- **APID Range**: 0-2047 (11-bit values)
- **Target Address**: Valid socket address format
- **Command Names**: Not empty
- **Command Count**: At least one command defined
- **Network Configuration**: Valid local/remote addresses

## Timing and Delays

Commands execute with the specified delays:

```toml
[[commands]]
name = "cmd1"
apid = 100
delay_ms = 0          # Execute immediately

[[commands]]
name = "cmd2"
apid = 101
delay_ms = 1000       # Wait 1 second before executing

[[commands]]
name = "cmd3"
apid = 102
delay_ms = 500        # Wait 0.5 seconds before executing
```

Total execution time = sum of all delays + network/processing overhead

## Logging

The framework uses the `log` crate for detailed logging:

```rust
// Enable logging in your main program
env_logger::init();

let mut engine = AutomationEngine::from_toml_file("config.toml")?;
engine.execute()?;  // Will output detailed logs
```

## Error Recovery

### Conditional Execution

Configure whether to continue on error:

```toml
stop_on_error = false  # Continue even if a command fails
```

### Retry Mechanism

Individual commands can be retried:

```toml
[[commands]]
name = "important_command"
apid = 100
data = [0x01]
retry_count = 3
```

## Performance Considerations

- **Blocking I/O**: Execution is synchronous and blocking
- **Memory**: Configuration is loaded entirely into memory
- **Delays**: Sleep-based; precision depends on OS scheduler
- **CRC Calculation**: Automatic, O(n) complexity

## Examples

See the following files for complete examples:

- `examples/ecss_automation_demo.rs` - 7 complete demonstrations
- `examples/ecss_automation_example.toml` - TOML configuration example
- `examples/ecss_automation_example.json` - JSON configuration example

## Testing

Run the automation tests:

```bash
cargo test --lib ecss_automation
```

15 comprehensive tests covering:
- Configuration parsing (TOML and JSON)
- Command validation
- Configuration validation
- Execution statistics
- Edge cases

## Troubleshooting

### Network Errors

If commands fail to send:
- Check the target address is correct
- Ensure the receiver is running
- Verify network connectivity
- Check firewall rules

### Validation Errors

If configuration fails to load:
- Verify APID values are 0-2047
- Check command names are not empty
- Ensure at least one command is defined
- Validate target address format (IP:PORT)

### Timeout Errors

If execution times out:
- Increase `timeout_ms`
- Reduce delays in commands
- Verify network performance
- Check for command processing issues

## Integration with ECSS UDP Library

This automation framework integrates seamlessly with the ECSS UDP library:

```rust
// The framework automatically uses the ECSS library
let mut engine = AutomationEngine::from_toml_file("config.toml")?;
// Behind the scenes:
// - Commands are converted to ECSS packets
// - CRC is automatically calculated
// - Packets are sent via UDP
let stats = engine.execute()?;
```

## Future Enhancements

Potential additions:
- Conditional command execution
- Command chaining and dependencies
- Real-time command injection
- Advanced scheduling (cron-like syntax)
- Command response validation
- Telemetry monitoring integration

## License

Part of the rust-and-vulkan project.

## Summary

The ECSS Command Automation Framework provides:

✓ Simple configuration file format (TOML or JSON)  
✓ Powerful command sequencing  
✓ Flexible timing and retry logic  
✓ Comprehensive error handling  
✓ Execution statistics and monitoring  
✓ Full integration with ECSS UDP library  
✓ Extensive validation  
✓ Metadata support for extensibility  

Use this framework to automate complex command sequences and simplify mission operations.
