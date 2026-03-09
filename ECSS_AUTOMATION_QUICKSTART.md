# ECSS Command Automation - Quick Start Guide

## 5-Minute Setup

### 1. Create a Configuration File

**`my_commands.toml`:**
```toml
target = "192.168.1.100:5000"
local = "0.0.0.0:0"
timeout_ms = 30000
stop_on_error = true

[[commands]]
name = "power_on"
apid = 100
delay_ms = 0
description = "Enable power"
data = [0x01, 0x00, 0xFF]
retry_count = 2

[[commands]]
name = "enable_antenna"
apid = 101
delay_ms = 1000
description = "Activate antenna"
data = [0x02, 0x01]
```

### 2. Execute in Code

```rust
use rust_and_vulkan::ecss_automation::AutomationEngine;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut engine = AutomationEngine::from_toml_file("my_commands.toml")?;
    let stats = engine.execute()?;
    
    println!("Success: {}/{}", stats.successful, stats.successful + stats.failed);
    println!("Rate: {:.1}%", stats.success_rate());
    
    Ok(())
}
```

## Configuration Format Comparison

### TOML Format

```toml
target = "192.168.1.100:5000"

[[commands]]
name = "cmd1"
apid = 100
data = [0x01, 0x02]
delay_ms = 500
retry_count = 2
```

### JSON Format

```json
{
  "target": "192.168.1.100:5000",
  "commands": [
    {
      "name": "cmd1",
      "apid": 100,
      "data": [1, 2],
      "delay_ms": 500,
      "retry_count": 2
    }
  ]
}
```

## Common Patterns

### Pattern 1: Simple Sequence

```toml
target = "192.168.1.100:5000"

[[commands]]
name = "initialize"
apid = 100
data = [0x01]

[[commands]]
name = "activate"
apid = 101
data = [0x02]
```

### Pattern 2: With Delays

```toml
[[commands]]
name = "step1"
apid = 100
data = [0x01]
delay_ms = 0

[[commands]]
name = "step2"
apid = 101
data = [0x02]
delay_ms = 1000  # Wait 1 second

[[commands]]
name = "step3"
apid = 102
data = [0x03]
delay_ms = 500
```

### Pattern 3: With Metadata

```toml
[[commands]]
name = "critical_command"
apid = 100
data = [0x01]
retry_count = 3

[commands.metadata]
priority = "critical"
subsystem = "Power"
```

### Pattern 4: Multiple Subsystems

```toml
[[commands]]
name = "power_on"
apid = 100  # Power subsystem
data = [0x01]

[[commands]]
name = "antenna_enable"
apid = 101  # Communications
data = [0x02]

[[commands]]
name = "thermal_setup"
apid = 102  # Thermal control
data = [0x03]
```

### Pattern 5: Error Handling

```toml
stop_on_error = false  # Continue on error
repeat_count = 1       # Don't repeat

[[commands]]
name = "optional_command"
apid = 100
data = [0x01]
retry_count = 0  # Don't retry
```

## Execution Examples

### From Code

```rust
use rust_and_vulkan::ecss_automation::{AutomationEngine, AutomationConfig, CommandDefinition};

// Method 1: From TOML file
let mut engine = AutomationEngine::from_toml_file("config.toml")?;

// Method 2: From JSON file
let mut engine = AutomationEngine::from_json_file("config.json")?;

// Method 3: From TOML string
let config_str = r#"target = "192.168.1.100:5000"
[[commands]]
name = "test"
apid = 100
data = [1]"#;
let mut engine = AutomationEngine::from_toml_str(config_str)?;

// Method 4: Programmatic
let config = AutomationConfig {
    target: "192.168.1.100:5000".to_string(),
    local: "0.0.0.0:0".to_string(),
    commands: vec![
        CommandDefinition::new("test".to_string(), 100, vec![0x01])
    ],
    timeout_ms: 30000,
    stop_on_error: true,
    repeat_count: 0,
};
let mut engine = AutomationEngine::new(config)?;

// Execute and get results
let stats = engine.execute()?;
println!("Successful: {}", stats.successful);
println!("Failed: {}", stats.failed);
println!("Rate: {:.1}%", stats.success_rate());
```

## Configuration Reference

### Top-Level Fields

```toml
target = "192.168.1.100:5000"      # Required: receiver address
local = "0.0.0.0:0"                 # Optional: local bind address
timeout_ms = 60000                  # Optional: max execution time
stop_on_error = true                # Optional: fail fast
repeat_count = 0                    # Optional: repeat sequence
commands = [...]                    # Required: command list
```

### Command Fields

```toml
[[commands]]
name = "command_name"               # Required: command identifier
apid = 100                          # Required: APID (0-2047)
data = [0x01, 0x02]                # Required: payload
delay_ms = 1000                     # Optional: delay (default: 0)
description = "What it does"        # Optional: documentation
retry_count = 2                     # Optional: retries (default: 0)
# metadata = {...}                  # Optional: custom fields
```

## Parameter Limits

| Parameter | Min | Max | Default |
|-----------|-----|-----|---------|
| APID | 0 | 2047 | - |
| delay_ms | 0 | 2^64-1 | 0 |
| retry_count | 0 | 255 | 0 |
| repeat_count | 0 | 255 | 0 |
| timeout_ms | 0 | 2^64-1 | 0 |
| sequence_counter | 0 | 16383 | auto |

## Validation Checks

The framework validates:

- ✓ APID range (0-2047)
- ✓ Target address format
- ✓ Command names not empty
- ✓ At least one command defined
- ✓ Valid socket addresses

## Error Handling

```rust
use rust_and_vulkan::ecss_automation::AutomationError;

match engine.execute() {
    Ok(stats) => {
        println!("Success rate: {:.1}%", stats.success_rate());
    }
    Err(AutomationError::ExecutionError(msg)) => {
        eprintln!("Execution failed: {}", msg);
    }
    Err(AutomationError::ValidationError(msg)) => {
        eprintln!("Validation failed: {}", msg);
    }
    Err(AutomationError::NetworkError(msg)) => {
        eprintln!("Network error: {}", msg);
    }
    Err(AutomationError::ConfigError(msg)) => {
        eprintln!("Configuration error: {}", msg);
    }
    Err(e) => {
        eprintln!("Error: {}", e);
    }
}
```

## Real-World Example

Satellite power-up sequence:

```toml
target = "192.168.100.1:5000"
timeout_ms = 120000
stop_on_error = true

# Step 1: Initialize power systems
[[commands]]
name = "enable_battery"
apid = 100
data = [0x01, 0xFF]
delay_ms = 0
retry_count = 3

[commands.metadata]
subsystem = "Power"
criticality = "critical"

# Step 2: Wait and enable other subsystems
[[commands]]
name = "enable_communications"
apid = 101
data = [0x02, 0x01]
delay_ms = 2000
retry_count = 2

[commands.metadata]
subsystem = "Comms"

# Step 3: Configure antenna
[[commands]]
name = "antenna_config"
apid = 102
data = [0x03, 0x45]
delay_ms = 1000

[commands.metadata]
subsystem = "Antenna"

# Step 4: Enable sensors
[[commands]]
name = "sensor_power"
apid = 200
data = [0x10, 0xFF]
delay_ms = 500
retry_count = 1

[commands.metadata]
subsystem = "Sensors"
```

## Tips & Tricks

### Tip 1: Start Simple
Create a minimal config first, then add complexity:
```toml
target = "192.168.1.100:5000"
[[commands]]
name = "test"
apid = 100
data = [1]
```

### Tip 2: Use Descriptions
Document what each command does:
```toml
description = "Power on main bus to 28V nominal"
```

### Tip 3: Set Realistic Delays
Account for subsystem processing times:
```toml
delay_ms = 1000  # 1 second for subsystem to respond
```

### Tip 4: Use Retry for Critical Commands
```toml
retry_count = 3  # Critical commands get 3 attempts
```

### Tip 5: Validate First
```rust
engine.config().validate()?;  // Check config before execution
```

## Common Issues

### Issue: Commands Not Executing

**Check:**
- Target address is correct
- Receiver is running and listening
- Network connectivity is good
- Firewall allows UDP on the port

### Issue: APID Out of Range

**Error:** "APID exceeds maximum of 2047"

**Fix:** APID must be 0-2047 (11-bit value)

### Issue: Invalid Address

**Error:** "Invalid target address"

**Fix:** Use format "IP:PORT" (e.g., "192.168.1.100:5000")

### Issue: No Commands

**Error:** "No commands defined"

**Fix:** Add at least one command to `[[commands]]`

### Issue: Timeout

**Error:** "Execution timeout exceeded"

**Fix:** Increase `timeout_ms` or reduce command delays

## Testing

Run tests:
```bash
cargo test --lib ecss_automation
```

Run example:
```bash
cargo run --example ecss_automation_demo
```

## Building Your Config

1. **Define subsystem APIDs** - Which APID for each subsystem?
2. **List operations** - What commands need to execute?
3. **Add timing** - When should each command execute?
4. **Set error handling** - Continue or stop on error?
5. **Test** - Run and monitor execution

## File Formats

### TOML: When to Use
- ✓ Human-readable
- ✓ Hierarchical structure
- ✓ Comments supported
- ✓ Good for manual editing

### JSON: When to Use
- ✓ Programmatic generation
- ✓ Integration with tools
- ✓ Standardized format
- ✓ Web API compatibility

## Next Steps

1. Create your `config.toml` or `config.json`
2. Update the target address for your system
3. List all commands you want to execute
4. Add appropriate delays between commands
5. Run with `AutomationEngine::from_toml_file()`
6. Monitor execution statistics

## Support Resources

- **Full Documentation**: See `ECSS_AUTOMATION_README.md`
- **Examples**: See `examples/ecss_automation_demo.rs`
- **Test Cases**: See `src/ecss_automation.rs` test module
- **UDP Library**: See `ECSS_UDP_README.md`

---

**Get Started**: Copy a configuration example and modify it for your use case!
