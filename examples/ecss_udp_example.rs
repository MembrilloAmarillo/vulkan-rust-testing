//! Example demonstrating ECSS-E-ST-70-41C UDP command library usage

// Uncomment this when you have a real ECSS receiver running
// use rust_and_vulkan::ecss_udp::{EcssUdpClient, TelecommandPacket, PacketType};

// For now, this example shows the library functionality
fn main() {
    println!("ECSS-E-ST-70-41C UDP Command Library Example\n");

    // Note: In a real scenario, you would use the library like this:
    /*
    // Create a UDP client
    let client = EcssUdpClient::new("0.0.0.0:0", "192.168.1.100:5000")
        .expect("Failed to create UDP client");

    // Create command data (example: command code + parameters)
    let command_data = vec![
        0x01, // Command code: Power on
        0x00, // Parameter 1: Device ID
        0x10, // Parameter 2: Power level
    ];

    // Send the command
    match client.send_command_data(100, &command_data, 0) {
        Ok(bytes_sent) => println!("Sent {} bytes", bytes_sent),
        Err(e) => println!("Error sending command: {}", e),
    }
    */

    // Example packet structure (for documentation)
    println!("Example ECSS Packet Structure:");
    println!("├── Primary Header (6 bytes)");
    println!("│   ├── Packet ID (2 bytes)");
    println!("│   │   ├── Version (3 bits)");
    println!("│   │   ├── Packet Type (1 bit: 0=telemetry, 1=telecommand)");
    println!("│   │   ├── Secondary Header Flag (1 bit)");
    println!("│   │   └── APID (11 bits)");
    println!("│   ├── Sequence Control (2 bytes)");
    println!("│   │   ├── Sequence Flag (2 bits)");
    println!("│   │   └── Sequence Counter (14 bits)");
    println!("│   └── Packet Data Length (2 bytes)");
    println!("├── Data Field (variable)");
    println!("└── CRC-16 (2 bytes)\n");

    println!("APID Range: 0-2047");
    println!("Sequence Counter Range: 0-16383");
    println!("Maximum data size: 65536 bytes\n");

    println!("To use this library in your project:");
    println!("1. Create an EcssUdpClient with local and remote addresses");
    println!("2. Prepare your command data");
    println!("3. Call send_command_data() or send_command() with your packet");
    println!("4. The library handles packet formation and CRC calculation");
}
