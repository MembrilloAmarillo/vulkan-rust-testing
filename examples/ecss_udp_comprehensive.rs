//! Comprehensive ECSS-E-ST-70-41C UDP command library examples

use rust_and_vulkan::ecss_udp::{
    EcssUdpClient, PacketIdentification, PacketType, PrimaryHeader, SequenceControl, SequenceFlag,
    TelecommandPacket,
};

/// Example 1: Simple command sending
fn example_simple_command() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n=== Example 1: Simple Command Sending ===");

    // Create a UDP client (note: requires a running ECSS receiver on the target address)
    // In practice, replace with your actual receiver address
    let client = EcssUdpClient::new("127.0.0.1:0", "127.0.0.1:5000")?;

    // Create a simple power-on command
    let power_on_command = vec![
        0x01, // Command ID: Power On
        0x00, // Device selector (all devices)
        0xFF, // Power level (max)
    ];

    // Send the command
    match client.send_command_data(100, &power_on_command, 0) {
        Ok(bytes) => println!("Successfully sent {} bytes", bytes),
        Err(e) => println!("Could not send (expected if no receiver): {}", e),
    }

    Ok(())
}

/// Example 2: Custom packet with full control
fn example_custom_packet() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n=== Example 2: Custom Packet Construction ===");

    // Create packet identification
    let packet_id = PacketIdentification::new(
        PacketType::Telecommand,
        256, // APID for subsystem control
        false,
    );

    // Create sequence control
    let seq_ctrl = SequenceControl::new(SequenceFlag::Unsegmented, 100);

    // Prepare payload: A firmware update command
    let payload = vec![
        0x05, // Command: Firmware Update
        0x01, // Update type: Full
        0x00, 0x10, // Start address (0x0010)
        0x00, 0x20, // Size (0x0020 bytes)
    ];

    // Create primary header
    let packet_length = payload.len() as u16 + 1;
    let primary_header = PrimaryHeader::new(packet_id, seq_ctrl, packet_length);

    // Create complete packet
    let packet = TelecommandPacket {
        primary_header,
        data: payload,
    };

    // Encode and inspect
    let encoded = packet.encode();
    println!("Total packet size: {} bytes", encoded.len());
    println!("  Header: 6 bytes");
    println!("  Data: {} bytes", encoded.len() - 8);
    println!("  CRC: 2 bytes");
    println!("CRC-16 value: 0x{:04X}", packet.calculate_crc());

    // Show hex representation
    println!("Hex representation: {}", hex_string(&encoded));

    Ok(())
}

/// Example 3: Multiple commands with sequence counting
fn example_sequence_counting() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n=== Example 3: Sequence Counting ===");

    // Simulate sending a sequence of commands
    let commands = vec![
        (0x01, vec![0x01, 0x00]),             // Initialize
        (0x02, vec![0x02, 0x10, 0x20]),       // Configure
        (0x03, vec![0x03, 0x55, 0xAA, 0xFF]), // Activate
    ];

    for (seq_count, payload) in commands {
        let packet = TelecommandPacket::new(200, seq_count as u16, payload.clone(), false);
        let encoded = packet.encode();
        println!(
            "Command {}: {} bytes (CRC: 0x{:04X})",
            seq_count,
            encoded.len(),
            packet.calculate_crc()
        );
    }

    Ok(())
}

/// Example 4: Different APID values for different subsystems
fn example_multisubsystem() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n=== Example 4: Multi-Subsystem Commands ===");

    let subsystems = vec![
        (100, "Power Management System"),
        (101, "Attitude Control System"),
        (102, "Communications Subsystem"),
        (103, "Thermal Control"),
        (200, "Science Payload"),
    ];

    for (apid, name) in subsystems {
        let packet = TelecommandPacket::new(apid, 0, vec![0x00, 0x00], false);
        let encoded = packet.encode();
        println!(
            "APID {}: {} (packet size: {} bytes)",
            apid,
            name,
            encoded.len()
        );
    }

    Ok(())
}

/// Example 5: Segmented packets (for large commands)
fn example_segmented_packets() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n=== Example 5: Segmented Packets ===");

    let large_payload: Vec<u8> = (0..100).map(|i| (i % 256) as u8).collect();

    // First segment
    let packet_id = PacketIdentification::new(PacketType::Telecommand, 150, false);
    let seq_ctrl_first = SequenceControl::new(SequenceFlag::First, 0);
    let ph_first = PrimaryHeader::new(packet_id.clone(), seq_ctrl_first, 50);
    let packet_first = TelecommandPacket {
        primary_header: ph_first,
        data: large_payload[0..50].to_vec(),
    };

    // Last segment
    let seq_ctrl_last = SequenceControl::new(SequenceFlag::Last, 1);
    let ph_last = PrimaryHeader::new(packet_id.clone(), seq_ctrl_last, 51);
    let packet_last = TelecommandPacket {
        primary_header: ph_last,
        data: large_payload[50..].to_vec(),
    };

    println!("First segment: {} bytes", packet_first.encode().len());
    println!("Last segment: {} bytes", packet_last.encode().len());
    println!(
        "Total payload: {} bytes (split across 2 packets)",
        large_payload.len()
    );

    Ok(())
}

/// Example 6: Demonstrate validation and error handling
fn example_validation() {
    println!("\n=== Example 6: Parameter Validation ===");

    // Valid APID
    println!("Valid APID (100): Creating packet...");
    let _pi_valid = PacketIdentification::new(PacketType::Telecommand, 100, false);
    println!("  ✓ Success");

    // Valid sequence count
    println!("Valid sequence count (1000): Creating control...");
    let _sc_valid = SequenceControl::new(SequenceFlag::Unsegmented, 1000);
    println!("  ✓ Success");

    // Invalid APID would panic
    println!("Invalid APID (2048): Attempting to create...");
    println!("  (Would panic - exceeds 11-bit limit)");

    // Invalid sequence count would panic
    println!("Invalid sequence count (16384): Attempting to create...");
    println!("  (Would panic - exceeds 14-bit limit)");
}

/// Example 7: Practical use case - satellite command sequence
fn example_satellite_operations() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n=== Example 7: Satellite Operations Sequence ===");

    struct SatelliteCommand {
        name: &'static str,
        apid: u16,
        payload: Vec<u8>,
    }

    let operations = vec![
        SatelliteCommand {
            name: "Enable Antenna",
            apid: 101,
            payload: vec![0x10, 0x01],
        },
        SatelliteCommand {
            name: "Set Transmission Power",
            apid: 101,
            payload: vec![0x11, 0x03, 0x50], // Channel 3, 50% power
        },
        SatelliteCommand {
            name: "Start Data Download",
            apid: 200,
            payload: vec![0x20, 0x00, 0x00, 0x00, 0x01], // From address 0, size 1
        },
    ];

    for (index, op) in operations.iter().enumerate() {
        let packet = TelecommandPacket::new(op.apid, index as u16, op.payload.clone(), false);
        let encoded = packet.encode();
        println!(
            "Step {}: {} (APID: {}, {} bytes)",
            index + 1,
            op.name,
            op.apid,
            encoded.len()
        );
    }

    Ok(())
}

/// Helper function to create hex string representation
fn hex_string(data: &[u8]) -> String {
    data.iter()
        .map(|b| format!("{:02X}", b))
        .collect::<Vec<_>>()
        .join(" ")
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("╔═══════════════════════════════════════════════════════════╗");
    println!("║  ECSS-E-ST-70-41C UDP Command Library - Examples         ║");
    println!("║  European Cooperation for Space Standardization           ║");
    println!("╚═══════════════════════════════════════════════════════════╝");

    // Run all examples
    example_simple_command()?;
    example_custom_packet()?;
    example_sequence_counting()?;
    example_multisubsystem()?;
    example_segmented_packets()?;
    example_validation();
    example_satellite_operations()?;

    println!("\n╔═══════════════════════════════════════════════════════════╗");
    println!("║  All examples completed successfully!                    ║");
    println!("╚═══════════════════════════════════════════════════════════╝\n");

    Ok(())
}
