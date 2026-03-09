# Automation File Loader

## Overview

The Automation File Loader is an integrated UI component that allows users to browse the filesystem, select automation configuration files (JSON, TOML, etc.), and view their content in a code editor within the application.

## Features

### File Browser Window
- **Directory Navigation**: Browse through the filesystem with parent directory navigation
- **File Filtering**: Filter files by extension (e.g., `.json`, `.toml`, `.txt`)
- **File Information**: Display file icons, names, and sizes in a sortable list
- **Refresh Capability**: Manual refresh button to reload directory contents
- **Error Handling**: Clear error messages for file access issues

### Code Display Window
- **Line Numbers**: Display line numbers for easy reference
- **Syntax Highlighting**: Monospace font with color-coded line numbers
- **Scrolling**: Vertical scrolling for large files
- **File Info**: Display currently loaded filename with 📄 icon

## Usage

### Opening the File Loader

The automation file loader is available through the "Automation File Loader" window in the egui UI. The window is shown by default and can be toggled on/off.

### Navigating Directories

1. **Parent Directory**: Click "↑ Parent Directory" to go up one level
2. **Subdirectories**: Click on any folder (📁 icon) to enter that directory
3. **Refresh**: Click "🔄 Refresh" to reload the current directory

### Filtering Files

Enter a file extension in the "Filter:" field:
- `.json` - JSON configuration files
- `.toml` - TOML configuration files
- `.txt` - Text command files
- `.rs` - Rust source files
- Empty filter shows all files

### Loading and Viewing Files

1. Click on a file (📄 icon) in the browser
2. The file content will automatically open in the "Code Display" window
3. View the code with line numbers and monospace formatting
4. Close the window with the "Close" button

## Architecture

### AutomationFileLoader Struct

Located in `src/automation.rs`, this struct manages:
- Current working directory
- File list with metadata
- Selected file and its content
- UI state (show/hide windows)
- Error messages and file filters

### Key Methods

- `new(start_dir: Option<PathBuf>)` - Create loader with optional starting directory
- `refresh_files()` - Reload file list from current directory
- `navigate_to(&Path)` - Navigate to a directory
- `navigate_up()` - Go to parent directory
- `load_file(&Path)` - Load and display file content
- `set_filter(String)` - Set file extension filter
- `format_size(u64)` - Human-readable file size formatting

### UI Integration

In `src/main.rs`, the file loader is integrated into the main render loop:

```rust
// Initialize
let mut automation_loader = AutomationFileLoader::default();

// Render windows
if automation_loader.show_browser {
    // File browser window UI
}

if automation_loader.show_code_display {
    // Code display window UI
}
```

## File Format: JSON Automation Configuration

The recommended format for automation files is JSON, which maps directly to ECSS commands:

```json
{
  "mission_name": "Example Spacecraft Mission",
  "description": "10-phase spacecraft control sequence",
  "total_duration_ms": 135000,
  "commands": [
    {
      "phase": 1,
      "description": "System Initialization",
      "command": {
        "apid": 6,
        "packet_id": 0,
        "name": "SYSTEM_SAFE_MODE_ENABLE"
      },
      "delay_ms": 1000
    },
    {
      "phase": 2,
      "description": "Configure Beacon Frequency",
      "command": {
        "apid": 6,
        "packet_id": 4,
        "name": "SYSTEM_CHANGE_BEACON_FREQUENCY",
        "parameters": {
          "frequency": 437500000
        }
      },
      "delay_ms": 1000
    },
    {
      "phase": 3,
      "description": "Boot Payload 1",
      "command": {
        "apid": 0,
        "packet_id": 0,
        "name": "PAY_1_BOOT"
      },
      "delay_ms": 2000
    }
  ]
}
```

### JSON Schema

- **mission_name** (string): Human-readable mission identifier
- **description** (string): Mission description
- **total_duration_ms** (integer): Estimated total execution time
- **commands** (array): List of command/delay steps

Each command entry contains:
- **phase** (integer): Mission phase number (for organization)
- **description** (string): What this step does
- **command** (object): The actual command to send
  - **apid** (integer): Application Process ID (0-5 for spacecraft commands)
  - **packet_id** (integer): Packet ID within that APID
  - **name** (string): Human-readable command name
  - **parameters** (object, optional): Command parameters
- **delay_ms** (integer): Wait time after this command (milliseconds)

## File Size Formatting

Files are displayed with human-readable sizes:
- `< 1 KB`: Bytes (e.g., "512 B")
- `< 1 MB`: Kilobytes (e.g., "1.50 KB")
- `< 1 GB`: Megabytes (e.g., "2.75 MB")
- `>= 1 GB`: Gigabytes (e.g., "1.25 GB")

## Example Automation File

See `example_automation.json` for a complete example that demonstrates:
- Spacecraft mode transitions (safe → operational → nominal)
- Beacon and baudrate configuration
- Payload initialization and measurement enabling
- Experiment timing and data collection
- EPS housekeeping queries
- Complete 10-phase mission sequence

## Error Handling

The file loader gracefully handles:
- Permission denied errors
- File not found errors
- Invalid directory paths
- Unreadable files

All errors are displayed in red text in the file browser window.

## Testing

Unit tests are included in `src/automation.rs`:
- `test_format_size()` - Verify human-readable size formatting
- `test_default_loader()` - Check default initialization
- `test_filter_files()` - Validate file filtering by extension

Run tests with:
```bash
cargo test --lib automation
```

## Future Enhancements

Potential improvements:
- JSON schema validation
- Command execution/validation
- Syntax highlighting for code display
- Recent files history
- File creation and editing
- Search/grep functionality
- Bookmark/favorite directories
- Multi-file selection for batch operations
- Parameter validation against command specs
