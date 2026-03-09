# ECSS-E-ST-70-41C UDP Command Library

A Rust library for encoding and sending commands via UDP following the European Cooperation for Space Standardization (ECSS) standard **ECSS-E-ST-70-41C: Telecommand and Telemetry Packet Utilization**.

## Overview

This library provides complete functionality for:
- Creating and encoding ECSS-compliant telecommand packets
- Calculating CRC-16-CCITT checksums
- Sending commands over UDP networks
- Handling packet structure validation

## Features

- **Full ECSS-E-ST-70-41C Compliance**: Implements the standard packet structure with primary headers, data fields, and error control
- **Type-Safe API**: Rust's type system ensures correct packet construction
- **Zero-Copy Operations**: Efficient packet encoding with minimal memory overhead
- **Error Handling**: Comprehensive validation of packet parameters
- **Well-Tested**: Includes unit tests for all packet components

## Packet Structure

According to ECSS-E-ST-70-41C, each packet consists of:

```
┌─────────────────────────────────────────────┐
│ Primary Header (6 bytes)                    │
├──────────────┬──────────────┬───────────────┤
│ Packet ID    │ Seq. Control │ Data Length   │
│ (2 bytes)    │ (2 bytes)    │ (2 bytes)     │
├─────────────────────────────────────────────┤
│ Data Field (variable length)                │
├─────────────────────────────────────────────┤
│ Error Control - CRC-16 (2 bytes)            │
└─────────────────────────────────────────────┘
```

### Primary Header Details

- **Packet ID (16 bits)**:
  - Version (3 bits): Typically 0
  - Packet Type (1 bit): 1 for Telecommand, 0 for Telemetry
  - Secondary Header Flag (1 bit): 1 if secondary header present
  - APID (11 bits): Application Process ID (0-2047)

- **Sequence Control (16 bits)**:
  - Sequence Flag (2 bits): 
    - 0: Continuation
    - 1: First segment
    - 2: Last segment
    - 3: Unsegmented (default for complete packets)
  - Sequence Counter (14 bits): 0-16383

- **Packet Data Length (16 bits)**: Length of data field + 1 (as per ECSS standard)

## Usage

### Basic Example

```rust
use rust_and_vulkan::ecss_udp::{EcssUdpClient, TelecommandPacket};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a UDP client
    let client = EcssUdpClient::new("0.0.0.0:0", "192.168.1.100:5000")?;
    
    // Create command data (e.g., command code + parameters)
    let command_data = vec![
        0x01,  // Command code: Power on
        0x00,  // Parameter 1: Device ID
        0x10,  // Parameter 2: Power level
    ];
    
    // Send the command
    let bytes_sent = client.send_command_data(
        100,              // APID
        &command_data,    // Command payload
        0,                // Sequence counter
    )?;
    
    println!("Sent {} bytes", bytes_sent);
    Ok(())
}
```

### Advanced Example - Custom Packet Construction

```rust
use rust_and_vulkan::ecss_udp::{
    PacketIdentification, PacketType, SequenceControl, SequenceFlag,
    PrimaryHeader, TelecommandPacket,
};

fn main() {
    // Create packet identification for a telecommand with APID 256
    let packet_id = PacketIdentification::new(
        PacketType::Telecommand,
        256,
        false,  // No secondary header
    );
    
    // Create sequence control (unsegmented packet, sequence count 42)
    let seq_control = SequenceControl::new(
        SequenceFlag::Unsegmented,
        42,
    );
    
    // Prepare command payload
    let payload = vec![0x42, 0x43, 0x44];
    
    // Create the primary header
    let packet_length = payload.len() as u16 + 1;
    let primary_header = PrimaryHeader::new(packet_id, seq_control, packet_length);
    
    // Create the complete packet
    let packet = TelecommandPacket {
        primary_header,
        data: payload,
    };
    
    // Encode to bytes
    let encoded = packet.encode();
    println!("Packet size: {} bytes", encoded.len());
    println!("Hex: {}", hex::encode(&encoded));
}
```

## API Reference

### Main Types

#### `EcssUdpClient`
UDP client for sending ECSS packets.

**Methods:**
- `new(local_addr: &str, remote_addr: &str)` - Create a new UDP client
- `send_command(packet: &TelecommandPacket)` - Send an encoded packet
- `send_command_data(apid: u16, data: &[u8], sequence_count: u16)` - Send raw command data
- `set_remote_address(remote_addr: &str)` - Change the remote address
- `remote_address()` - Get the current remote address

#### `TelecommandPacket`
Complete ECSS telecommand packet.

**Fields:**
- `primary_header: PrimaryHeader` - The packet header
- `data: Vec<u8>` - The command payload

**Methods:**
- `new(apid: u16, sequence_count: u16, data: Vec<u8>, has_secondary_header: bool)` - Create packet
- `encode()` - Encode to bytes (header + data + CRC)
- `calculate_crc()` - Calculate CRC-16-CCITT checksum

#### `PacketIdentification`
Identifies packet type and destination.

**Fields:**
- `version: u8` - Packet version (typically 0)
- `packet_type: PacketType` - Telecommand or Telemetry
- `data_field_header: bool` - Has secondary header
- `apid: u16` - Application Process ID (0-2047)

#### `SequenceControl`
Packet sequence and segmentation control.

**Fields:**
- `sequence_flag: SequenceFlag` - Segment type
- `sequence_count: u16` - Packet sequence number (0-16383)

#### `PrimaryHeader`
ECSS standard primary header.

**Fields:**
- `packet_id: PacketIdentification` - Packet identification
- `sequence_control: SequenceControl` - Sequence information
- `packet_length: u16` - Data length (length - 1)

### Enumerations

#### `PacketType`
- `Telecommand` = 1
- `Telemetry` = 0

#### `SequenceFlag`
- `Continuation` = 0 (Continuation segment)
- `First` = 1 (First segment)
- `Last` = 2 (Last segment)
- `Unsegmented` = 3 (Complete packet)

## Error Handling

The library validates parameters and returns errors for:
- APID out of range (must be 0-2047)
- Sequence count out of range (must be 0-16383)
- Network binding failures
- Invalid address strings

## CRC Calculation

The library uses CRC-16-CCITT with:
- Polynomial: 0x1021
- Initial value: 0xFFFF
- Applied to: Header + Data

## Testing

Run the comprehensive test suite:

```bash
cargo test --lib ecss_udp
```

Tests cover:
- Packet identification encoding
- Sequence control encoding
- Primary header encoding
- Telecommand packet creation
- CRC calculation
- Parameter validation
- Boundary conditions

## ECSS Standard Reference

- **Standard**: ECSS-E-ST-70-41C
- **Title**: Telecommand and Telemetry Packet Utilization
- **Organization**: European Cooperation for Space Standardization (ECSS)
- **Scope**: Space systems telemetry and telecommand packet structures

## Integration Notes

To use in your project:

1. Add the module to your `lib.rs`:
   ```rust
   pub mod ecss_udp;
   ```

2. Import the types:
   ```rust
   use your_crate::ecss_udp::{EcssUdpClient, TelecommandPacket};
   ```

3. Build and test:
   ```bash
   cargo build
   cargo test
   ```

## Performance

- Packet encoding: O(n) where n is data size
- CRC calculation: O(n) where n is header + data size
- Network send: Depends on UDP implementation
- Memory overhead: Minimal with zero-copy operations

## License

Included as part of the rust-and-vulkan project.

## Future Enhancements

Potential future additions:
- Telemetry packet decoding
- Secondary header support
- Advanced CRC options
- Async UDP operations
- Packet segmentation handling
- Command result parsing utilities
