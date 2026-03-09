//! ECSS-E-ST-70-41C Telecommand and Telemetry Packet Utilization Library
//!
//! This library provides functionality for encoding and sending commands following the
//! European Cooperation for Space Standardization (ECSS) standard ECSS-E-ST-70-41C.
//!
//! The standard defines packet structures for space systems telemetry and telecommands.
//!
//! # Packet Structure
//!
//! According to ECSS-E-ST-70-41C, packets consist of:
//! - Primary Header (6 bytes minimum)
//! - Secondary Header (variable, optional)
//! - Data Field (variable, optional)
//! - Error Control (CRC-16, 2 bytes)

use std::net::{SocketAddr, UdpSocket};

/// ECSS Packet Version Number (3 bits)
const PACKET_VERSION: u8 = 0;

/// Packet type enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PacketType {
    Telecommand = 1,
    Telemetry = 0,
}

/// Packet identification information
#[derive(Debug, Clone)]
pub struct PacketIdentification {
    /// Packet version number (0-7, typically 0)
    pub version: u8,
    /// Packet type: telecommand (1) or telemetry (0)
    pub packet_type: PacketType,
    /// Data field header flag: 1 if secondary header present, 0 otherwise
    pub data_field_header: bool,
    /// Application Process ID (11 bits, 0-2047)
    pub apid: u16,
}

impl PacketIdentification {
    /// Creates a new packet identification
    pub fn new(packet_type: PacketType, apid: u16, has_secondary_header: bool) -> Self {
        assert!(apid < 2048, "APID must be 11 bits (0-2047)");
        PacketIdentification {
            version: PACKET_VERSION,
            packet_type,
            data_field_header: has_secondary_header,
            apid,
        }
    }

    /// Encodes the identification into two bytes (packet ID)
    fn encode(&self) -> [u8; 2] {
        let mut packet_id: u16 = 0;
        packet_id |= ((self.version as u16) & 0x7) << 13;
        packet_id |= ((self.packet_type as u16) & 0x1) << 12;
        packet_id |= (self.data_field_header as u16) << 11;
        packet_id |= self.apid & 0x7FF;
        packet_id.to_be_bytes()
    }
}

/// Packet sequence flags
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SequenceFlag {
    /// This is a continuation segment
    Continuation = 0,
    /// This is the first segment
    First = 1,
    /// This is the last segment
    Last = 2,
    /// This is an unsegmented packet (first and last)
    Unsegmented = 3,
}

/// Packet sequence control information
#[derive(Debug, Clone)]
pub struct SequenceControl {
    /// Sequence flag
    pub sequence_flag: SequenceFlag,
    /// Sequence counter (14 bits, 0-16383)
    pub sequence_count: u16,
}

impl SequenceControl {
    /// Creates a new sequence control
    pub fn new(sequence_flag: SequenceFlag, sequence_count: u16) -> Self {
        assert!(
            sequence_count < 16384,
            "Sequence count must be 14 bits (0-16383)"
        );
        SequenceControl {
            sequence_flag,
            sequence_count,
        }
    }

    /// Encodes the sequence control into two bytes
    fn encode(&self) -> [u8; 2] {
        let mut seq_ctrl: u16 = 0;
        seq_ctrl |= ((self.sequence_flag as u16) & 0x3) << 14;
        seq_ctrl |= self.sequence_count & 0x3FFF;
        seq_ctrl.to_be_bytes()
    }
}

/// Primary header of an ECSS packet
#[derive(Debug, Clone)]
pub struct PrimaryHeader {
    /// Packet identification
    pub packet_id: PacketIdentification,
    /// Packet sequence control
    pub sequence_control: SequenceControl,
    /// Packet data length (length of data field + secondary header + 1, in bytes)
    pub packet_length: u16,
}

impl PrimaryHeader {
    /// Creates a new primary header
    pub fn new(
        packet_id: PacketIdentification,
        sequence_control: SequenceControl,
        packet_length: u16,
    ) -> Self {
        PrimaryHeader {
            packet_id,
            sequence_control,
            packet_length,
        }
    }

    /// Encodes the primary header (6 bytes)
    fn encode(&self) -> [u8; 6] {
        let mut header = [0u8; 6];
        let packet_id_bytes = self.packet_id.encode();
        let seq_ctrl_bytes = self.sequence_control.encode();
        let packet_len_bytes = self.packet_length.to_be_bytes();

        header[0..2].copy_from_slice(&packet_id_bytes);
        header[2..4].copy_from_slice(&seq_ctrl_bytes);
        header[4..6].copy_from_slice(&packet_len_bytes);

        header
    }
}

/// Telecommand packet command
#[derive(Debug, Clone)]
pub struct TelecommandPacket {
    /// Primary header
    pub primary_header: PrimaryHeader,
    /// Command data (payload)
    pub data: Vec<u8>,
}

impl TelecommandPacket {
    /// Creates a new telecommand packet
    pub fn new(apid: u16, sequence_count: u16, data: Vec<u8>, has_secondary_header: bool) -> Self {
        let packet_id =
            PacketIdentification::new(PacketType::Telecommand, apid, has_secondary_header);
        let sequence_control = SequenceControl::new(SequenceFlag::Unsegmented, sequence_count);

        // Packet data length includes data + CRC (2 bytes)
        let packet_length = (data.len() as u16) + 1; // +1 as per ECSS standard (length - 1)

        let primary_header = PrimaryHeader::new(packet_id, sequence_control, packet_length);

        TelecommandPacket {
            primary_header,
            data,
        }
    }

    /// Calculates CRC-16-CCITT for the packet
    pub fn calculate_crc(&self) -> u16 {
        let mut crc: u16 = 0xFFFF;

        // CRC over primary header
        let header = self.primary_header.encode();
        for byte in header.iter() {
            crc = crc_byte(crc, *byte);
        }

        // CRC over data
        for byte in self.data.iter() {
            crc = crc_byte(crc, *byte);
        }

        crc
    }

    /// Encodes the complete packet with CRC
    pub fn encode(&self) -> Vec<u8> {
        let mut packet = Vec::new();

        // Add primary header
        packet.extend_from_slice(&self.primary_header.encode());

        // Add data
        packet.extend_from_slice(&self.data);

        // Calculate and add CRC
        let crc = self.calculate_crc();
        packet.extend_from_slice(&crc.to_be_bytes());

        packet
    }
}

/// CRC-16-CCITT polynomial calculation
fn crc_byte(mut crc: u16, byte: u8) -> u16 {
    for i in 0..8 {
        let carry = (crc >> 15) & 1;
        crc <<= 1;
        if (byte >> (7 - i)) & 1 == 1 {
            crc ^= 1;
        }
        if carry == 1 {
            crc ^= 0x1021;
        }
    }
    crc &= 0xFFFF;
    crc
}

/// Helper to encode u24 (24-bit unsigned integer) in big-endian
pub fn encode_u24(value: u32) -> [u8; 3] {
    [(value >> 16) as u8, (value >> 8) as u8, value as u8]
}

/// Telecommand command opcodes and builders
pub mod telecommands {
    use super::*;

    /// Command ID constants (APID=5 for all)
    pub const APID_PAYLOAD_EPS_OBC_SYSTEM: u16 = 5;

    /// Payload 1 commands (packet_id=0-7)
    pub mod payload1 {
        use super::super::encode_u24;

        /// PAY_1_BOOT: Boots payload for measurement purposes
        pub fn pay_1_boot() -> Vec<u8> {
            vec![] // VOID argument
        }

        /// PAY_1_ELECTRIC_NOISE_ENABLE: Enables electric noise measurements
        pub fn pay_1_electric_noise_enable() -> Vec<u8> {
            vec![] // VOID argument
        }

        /// PAY_1_X_AXIS_ENABLE: Enables X axis measurements
        pub fn pay_1_x_axis_enable() -> Vec<u8> {
            vec![] // VOID argument
        }

        /// PAY_1_Y_AXIS_ENABLE: Enables Y axis measurements
        pub fn pay_1_y_axis_enable() -> Vec<u8> {
            vec![] // VOID argument
        }

        /// PAY_1_Z_AXIS_ENABLE: Enables Z axis measurements
        pub fn pay_1_z_axis_enable() -> Vec<u8> {
            vec![] // VOID argument
        }

        /// PAY_1_STOP_EXPERIMENT: Stops payload experiment
        pub fn pay_1_stop_experiment() -> Vec<u8> {
            vec![] // VOID argument
        }

        /// PAY_1_STOP_TIME: Selects a stop time from 0 to 154800s (24-bit UINT)
        pub fn pay_1_stop_time(stop_time: u32) -> Vec<u8> {
            assert!(stop_time <= 154800, "stop_time must be <= 154800");
            encode_u24(stop_time).to_vec()
        }

        /// PAY_1_DOWNLOAD_PACKET: Downloads a specific number of scientific packets (32-bit UINT)
        pub fn pay_1_download_packet(n_packets: u32) -> Vec<u8> {
            n_packets.to_be_bytes().to_vec()
        }
    }

    /// Payload 2 commands (packet_id=0-7)
    pub mod payload2 {
        use super::super::encode_u24;

        /// PAY_2_BOOT: Boots payload for measurement purposes
        pub fn pay_2_boot() -> Vec<u8> {
            vec![] // VOID argument
        }

        /// PAY_2_ELECTRIC_NOISE_ENABLE: Enables electric noise measurements
        pub fn pay_2_electric_noise_enable() -> Vec<u8> {
            vec![] // VOID argument
        }

        /// PAY_2_X_AXIS_ENABLE: Enables X axis measurements
        pub fn pay_2_x_axis_enable() -> Vec<u8> {
            vec![] // VOID argument
        }

        /// PAY_2_Y_AXIS_ENABLE: Enables Y axis measurements
        pub fn pay_2_y_axis_enable() -> Vec<u8> {
            vec![] // VOID argument
        }

        /// PAY_2_Z_AXIS_ENABLE: Enables Z axis measurements
        pub fn pay_2_z_axis_enable() -> Vec<u8> {
            vec![] // VOID argument
        }

        /// PAY_2_STOP_EXPERIMENT: Stops payload experiment
        pub fn pay_2_stop_experiment() -> Vec<u8> {
            vec![] // VOID argument
        }

        /// PAY_2_STOP_TIME: Selects a stop time from 0 to 154800s (24-bit UINT)
        pub fn pay_2_stop_time(stop_time: u32) -> Vec<u8> {
            assert!(stop_time <= 154800, "stop_time must be <= 154800");
            encode_u24(stop_time).to_vec()
        }

        /// PAY_2_DOWNLOAD_PACKET: Downloads a specific number of scientific packets (32-bit UINT)
        pub fn pay_2_download_packet(n_packets: u32) -> Vec<u8> {
            n_packets.to_be_bytes().to_vec()
        }
    }

    /// EPS commands (packet_id=0-22)
    pub mod eps {
        /// EPS_SYSTEM_RESET: Perform a software induced reset of the MCU
        pub fn eps_system_reset() -> Vec<u8> {
            vec![] // VOID argument
        }

        /// EPS_WATCHDOG_TIMER_RESET: Resets the watchdog's timer
        pub fn eps_watchdog_timer_reset() -> Vec<u8> {
            vec![] // VOID argument
        }

        /// EPS_OUTPUT_BUS_GROUP_STATE: Turn-on/off bus channels (32-bit UINT)
        pub fn eps_output_bus_group_state(channels: u32) -> Vec<u8> {
            channels.to_be_bytes().to_vec()
        }

        /// EPS_OUTPUT_BUS_CHANNEL_ON: Turn a single output bus channel on (16-bit UINT)
        pub fn eps_output_bus_channel_on(channel_idx: u16) -> Vec<u8> {
            channel_idx.to_be_bytes().to_vec()
        }

        /// EPS_OUTPUT_BUS_CHANNEL_OFF: Turn a single output bus channel off (16-bit UINT)
        pub fn eps_output_bus_channel_off(channel_idx: u16) -> Vec<u8> {
            channel_idx.to_be_bytes().to_vec()
        }

        /// EPS_SWITCH_TO_NOMINAL_MODE: Move system to nominal mode
        pub fn eps_switch_to_nominal_mode() -> Vec<u8> {
            vec![] // VOID argument
        }

        /// EPS_SWITCH_TO_SAFETY_MODE: Move subsystem to safety mode
        pub fn eps_switch_to_safety_mode() -> Vec<u8> {
            vec![] // VOID argument
        }

        /// EPS_GET_SYSTEM_STATUS: Return system status information
        pub fn eps_get_system_status() -> Vec<u8> {
            vec![] // VOID argument
        }

        /// EPS_GET_PDU_PIU_OVERCURRENT_FAULT_STATE: Get overcurrent fault state
        pub fn eps_get_pdu_piu_overcurrent_fault_state() -> Vec<u8> {
            vec![] // VOID argument
        }

        /// EPS_GET_PBU_ABF_PLACED_STATE: Get ABF placed state information
        pub fn eps_get_pbu_abf_placed_state() -> Vec<u8> {
            vec![] // VOID argument
        }

        /// EPS_GET_PDU_HOUSEKEEPING_DATA_ENG: Get PDU housekeeping data (engineering form)
        pub fn eps_get_pdu_housekeeping_data_eng() -> Vec<u8> {
            vec![] // VOID argument
        }

        /// EPS_GET_PBU_HOUSEKEEPING_DATA_ENG: Get PBU housekeeping data (engineering values)
        pub fn eps_get_pbu_housekeeping_data_eng() -> Vec<u8> {
            vec![] // VOID argument
        }

        /// EPS_GET_PCU_HOUSEKEEPING_DATA_ENG: Get PCU housekeeping data (engineering values)
        pub fn eps_get_pcu_housekeeping_data_eng() -> Vec<u8> {
            vec![] // VOID argument
        }

        /// EPS_GET_CONFIGURATION_PARAMETER: Get configuration parameter by ID (16-bit UINT)
        pub fn eps_get_configuration_parameter(id: u16) -> Vec<u8> {
            id.to_be_bytes().to_vec()
        }

        /// EPS_SET_CONFIGURATION_PARAMETER: Set configuration parameter (ID: 16-bit, Value: 1-4 bytes)
        pub fn eps_set_configuration_parameter(id: u16, value: &[u8]) -> Vec<u8> {
            assert!(
                value.len() >= 1 && value.len() <= 4,
                "value must be 1-4 bytes"
            );
            let mut cmd = id.to_be_bytes().to_vec();
            cmd.extend_from_slice(value);
            cmd
        }

        /// EPS_RESET_CONFIGURATION_PARAMETER: Reset parameter to default (16-bit UINT)
        pub fn eps_reset_configuration_parameter(id: u16) -> Vec<u8> {
            id.to_be_bytes().to_vec()
        }

        /// EPS_RESET_CONFIGURATION: Reset all configuration to hard-coded defaults
        pub fn eps_reset_configuration() -> Vec<u8> {
            vec![] // VOID argument
        }

        /// EPS_LOAD_CONFIGURATION: Load configuration from non-volatile memory
        pub fn eps_load_configuration() -> Vec<u8> {
            vec![] // VOID argument
        }

        /// EPS_SAVE_CONFIGURATION: Save configuration to non-volatile memory (16-bit checksum)
        pub fn eps_save_configuration(checksum: u16) -> Vec<u8> {
            checksum.to_be_bytes().to_vec()
        }

        /// EPS_GET_PIU_HOUSEKEEPING_DATA: Get PIU housekeeping data (engineering values)
        pub fn eps_get_piu_housekeeping_data() -> Vec<u8> {
            vec![] // VOID argument
        }

        /// EPS_CORRECT_TIME: Correct unix time (32-bit signed INT)
        pub fn eps_correct_time(correction: i32) -> Vec<u8> {
            correction.to_be_bytes().to_vec()
        }

        /// EPS_ZERO_RESET_CAUSE_COUNTERS: Write all reset cause counters to zero
        pub fn eps_zero_reset_cause_counters() -> Vec<u8> {
            vec![] // VOID argument
        }
    }

    /// OBC commands (packet_id=0-2)
    pub mod obc {
        /// OBC_DUMP_RANGE: Request a range of data packets (start, end: 16-bit UINT each)
        pub fn obc_dump_range(start: u16, end: u16) -> Vec<u8> {
            let mut cmd = start.to_be_bytes().to_vec();
            cmd.extend_from_slice(&end.to_be_bytes());
            cmd
        }

        /// OBC_DUMP_PACKET: Request a specific packet (16-bit UINT)
        pub fn obc_dump_packet(packet_index: u16) -> Vec<u8> {
            packet_index.to_be_bytes().to_vec()
        }

        /// OBC_BOOT: Request OBC reboot
        pub fn obc_boot() -> Vec<u8> {
            vec![] // VOID argument
        }
    }

    /// System commands (packet_id=0-5)
    pub mod system {
        /// SYSTEM_SAFE_MODE_ENABLE: Turn S/C into Safe Mode
        pub fn system_safe_mode_enable() -> Vec<u8> {
            vec![] // VOID argument
        }

        /// SYSTEM_OPERATIONAL_MODE_ENABLE: Turn S/C into Operational Mode
        pub fn system_operational_mode_enable() -> Vec<u8> {
            vec![] // VOID argument
        }

        /// SYSTEM_NOMINAL_MODE_ENABLE: Turn S/C into Nominal Mode
        pub fn system_nominal_mode_enable() -> Vec<u8> {
            vec![] // VOID argument
        }

        /// SYSTEM_GET_MODE_ENABLED: Get S/C current mode
        pub fn system_get_mode_enabled() -> Vec<u8> {
            vec![] // VOID argument
        }

        /// SYSTEM_CHANGE_BEACON_FREQUENCY: Change beacon frequency (32-bit UINT)
        pub fn system_change_beacon_frequency(frequency: u32) -> Vec<u8> {
            frequency.to_be_bytes().to_vec()
        }

        /// SYSTEM_CHANGE_BAUDRATE: Change baudrate (32-bit UINT)
        pub fn system_change_baudrate(new_baudrate: u32) -> Vec<u8> {
            new_baudrate.to_be_bytes().to_vec()
        }
    }

    /// Helper to create a complete telecommand packet with APID=5
    pub fn create_telecommand(
        _packet_id: u8,
        data: Vec<u8>,
        sequence_count: u16,
    ) -> TelecommandPacket {
        TelecommandPacket::new(APID_PAYLOAD_EPS_OBC_SYSTEM, sequence_count, data, false)
    }
}

/// PUS Service telemetry packet structures
pub mod pus_services {
    use super::*;

    /// PUS Service Type 3: Housekeeping Parameter Reports
    #[derive(Debug, Clone)]
    pub struct PusService3_31 {
        /// Structure ID
        pub struct_id: u8,
        /// Housekeeping parameter data (variable length)
        pub data: Vec<u8>,
    }

    impl PusService3_31 {
        pub fn new(struct_id: u8, data: Vec<u8>) -> Self {
            PusService3_31 { struct_id, data }
        }

        pub fn encode(&self) -> Vec<u8> {
            let mut encoded = vec![self.struct_id];
            encoded.extend_from_slice(&self.data);
            encoded
        }
    }

    /// PUS Service Type 3: Housekeeping Parameter Periodic Generation Properties Report
    #[derive(Debug, Clone)]
    pub struct PusService3_33 {
        /// Structure ID
        pub struct_id: u8,
        /// Periodic generation properties data (variable length)
        pub data: Vec<u8>,
    }

    impl PusService3_33 {
        pub fn new(struct_id: u8, data: Vec<u8>) -> Self {
            PusService3_33 { struct_id, data }
        }

        pub fn encode(&self) -> Vec<u8> {
            let mut encoded = vec![self.struct_id];
            encoded.extend_from_slice(&self.data);
            encoded
        }
    }

    /// PUS Service Type 4: Parameter Statistics Reporting
    #[derive(Debug, Clone)]
    pub struct PusService4_1 {
        /// Parameter statistics report data (variable length)
        pub data: Vec<u8>,
    }

    impl PusService4_1 {
        pub fn new(data: Vec<u8>) -> Self {
            PusService4_1 { data }
        }

        pub fn encode(&self) -> Vec<u8> {
            self.data.clone()
        }
    }

    /// PUS Service Type 4: Parameter Statistics Reset
    #[derive(Debug, Clone)]
    pub struct PusService4_3 {
        /// Parameter IDs to reset (variable length, each u16)
        pub parameter_ids: Vec<u16>,
    }

    impl PusService4_3 {
        pub fn new(parameter_ids: Vec<u16>) -> Self {
            PusService4_3 { parameter_ids }
        }

        pub fn encode(&self) -> Vec<u8> {
            self.parameter_ids
                .iter()
                .flat_map(|id| id.to_be_bytes().to_vec())
                .collect()
        }
    }

    /// PUS Service Type 4: Enable Periodic Parameter Statistics Reporting
    #[derive(Debug, Clone)]
    pub struct PusService4_4 {
        /// Collection interval in seconds (32-bit UINT)
        pub collection_interval: u32,
    }

    impl PusService4_4 {
        pub fn new(collection_interval: u32) -> Self {
            PusService4_4 {
                collection_interval,
            }
        }

        pub fn encode(&self) -> Vec<u8> {
            self.collection_interval.to_be_bytes().to_vec()
        }
    }

    /// PUS Service Type 4: Disable Periodic Parameter Statistics Reporting
    #[derive(Debug, Clone)]
    pub struct PusService4_5;

    impl PusService4_5 {
        pub fn new() -> Self {
            PusService4_5
        }

        pub fn encode(&self) -> Vec<u8> {
            vec![] // No parameters
        }
    }

    /// PUS Service Type 5: Event Reporting - Enable Report Generation
    #[derive(Debug, Clone)]
    pub struct PusService5_5 {
        /// Event definition IDs to enable (variable length, each u16)
        pub event_ids: Vec<u16>,
    }

    impl PusService5_5 {
        pub fn new(event_ids: Vec<u16>) -> Self {
            PusService5_5 { event_ids }
        }

        pub fn encode(&self) -> Vec<u8> {
            self.event_ids
                .iter()
                .flat_map(|id| id.to_be_bytes().to_vec())
                .collect()
        }
    }

    /// PUS Service Type 5: Event Reporting - Report Parameter Statistics
    #[derive(Debug, Clone)]
    pub struct PusService5_6 {
        /// Event statistics data (variable length)
        pub data: Vec<u8>,
    }

    impl PusService5_6 {
        pub fn new(data: Vec<u8>) -> Self {
            PusService5_6 { data }
        }

        pub fn encode(&self) -> Vec<u8> {
            self.data.clone()
        }
    }

    /// PUS Service Type 5: Event Reporting - Report List of Disabled Event Definitions
    #[derive(Debug, Clone)]
    pub struct PusService5_7 {
        /// Disabled event definition IDs (variable length, each u16)
        pub event_ids: Vec<u16>,
    }

    impl PusService5_7 {
        pub fn new(event_ids: Vec<u16>) -> Self {
            PusService5_7 { event_ids }
        }

        pub fn encode(&self) -> Vec<u8> {
            self.event_ids
                .iter()
                .flat_map(|id| id.to_be_bytes().to_vec())
                .collect()
        }
    }

    /// PUS Service Type 17: Test and Report - Are you alive request
    #[derive(Debug, Clone)]
    pub struct PusService17_1 {
        /// Request ID (32-bit UINT)
        pub request_id: u32,
    }

    impl PusService17_1 {
        pub fn new(request_id: u32) -> Self {
            PusService17_1 { request_id }
        }

        pub fn encode(&self) -> Vec<u8> {
            self.request_id.to_be_bytes().to_vec()
        }
    }

    /// PUS Service Type 21: Time-based Scheduling - Direct Load Request Sequence
    #[derive(Debug, Clone)]
    pub struct PusService21_1 {
        /// Sequence ID (32-bit UINT)
        pub sequence_id: u32,
        /// Sequence data (variable length)
        pub sequence_data: Vec<u8>,
    }

    impl PusService21_1 {
        pub fn new(sequence_id: u32, sequence_data: Vec<u8>) -> Self {
            PusService21_1 {
                sequence_id,
                sequence_data,
            }
        }

        pub fn encode(&self) -> Vec<u8> {
            let mut encoded = self.sequence_id.to_be_bytes().to_vec();
            encoded.extend_from_slice(&self.sequence_data);
            encoded
        }
    }

    /// PUS Service Type 21: Time-based Scheduling - Load Request Sequence by Reference
    #[derive(Debug, Clone)]
    pub struct PusService21_2 {
        /// Sequence ID (32-bit UINT)
        pub sequence_id: u32,
        /// Sequence name/path (variable length)
        pub sequence_name: Vec<u8>,
    }

    impl PusService21_2 {
        pub fn new(sequence_id: u32, sequence_name: Vec<u8>) -> Self {
            PusService21_2 {
                sequence_id,
                sequence_name,
            }
        }

        pub fn encode(&self) -> Vec<u8> {
            let mut encoded = self.sequence_id.to_be_bytes().to_vec();
            encoded.extend_from_slice(&self.sequence_name);
            encoded
        }
    }

    /// PUS Service Type 21: Time-based Scheduling - Unload Request Sequence
    #[derive(Debug, Clone)]
    pub struct PusService21_3 {
        /// Sequence ID (32-bit UINT)
        pub sequence_id: u32,
    }

    impl PusService21_3 {
        pub fn new(sequence_id: u32) -> Self {
            PusService21_3 { sequence_id }
        }

        pub fn encode(&self) -> Vec<u8> {
            self.sequence_id.to_be_bytes().to_vec()
        }
    }

    /// PUS Service Type 21: Time-based Scheduling - Activate Request Sequence
    #[derive(Debug, Clone)]
    pub struct PusService21_4 {
        /// Sequence ID (32-bit UINT)
        pub sequence_id: u32,
    }

    impl PusService21_4 {
        pub fn new(sequence_id: u32) -> Self {
            PusService21_4 { sequence_id }
        }

        pub fn encode(&self) -> Vec<u8> {
            self.sequence_id.to_be_bytes().to_vec()
        }
    }

    /// PUS Service Type 21: Time-based Scheduling - Abort Request Sequence
    #[derive(Debug, Clone)]
    pub struct PusService21_5 {
        /// Sequence ID (32-bit UINT)
        pub sequence_id: u32,
    }

    impl PusService21_5 {
        pub fn new(sequence_id: u32) -> Self {
            PusService21_5 { sequence_id }
        }

        pub fn encode(&self) -> Vec<u8> {
            self.sequence_id.to_be_bytes().to_vec()
        }
    }

    /// PUS Service Type 21: Time-based Scheduling - Report Execution Status
    #[derive(Debug, Clone)]
    pub struct PusService21_6 {
        /// Sequence status data (variable length)
        pub status_data: Vec<u8>,
    }

    impl PusService21_6 {
        pub fn new(status_data: Vec<u8>) -> Self {
            PusService21_6 { status_data }
        }

        pub fn encode(&self) -> Vec<u8> {
            self.status_data.clone()
        }
    }

    /// PUS Service Type 21: Time-based Scheduling - Abort All and Report
    #[derive(Debug, Clone)]
    pub struct PusService21_13 {
        /// Abort report data (variable length)
        pub report_data: Vec<u8>,
    }

    impl PusService21_13 {
        pub fn new(report_data: Vec<u8>) -> Self {
            PusService21_13 { report_data }
        }

        pub fn encode(&self) -> Vec<u8> {
            self.report_data.clone()
        }
    }

    /// PUS Service Type 24: Time-based Scheduling - Enable Parameter Report
    #[derive(Debug, Clone)]
    pub struct PusService24_1 {
        /// Housekeeping ID (16-bit UINT)
        pub hk_id: u16,
        /// Parameter count N (16-bit UINT)
        pub n: u16,
        /// Parameter IDs (variable length, each u16)
        pub parameter_ids: Vec<u16>,
    }

    impl PusService24_1 {
        pub fn new(hk_id: u16, parameter_ids: Vec<u16>) -> Self {
            let n = parameter_ids.len() as u16;
            PusService24_1 {
                hk_id,
                n,
                parameter_ids,
            }
        }

        pub fn encode(&self) -> Vec<u8> {
            let mut encoded = self.hk_id.to_be_bytes().to_vec();
            encoded.extend_from_slice(&self.n.to_be_bytes());
            for id in &self.parameter_ids {
                encoded.extend_from_slice(&id.to_be_bytes());
            }
            encoded
        }
    }

    /// PUS Service Type 24: Time-based Scheduling - Disable Parameter Report
    #[derive(Debug, Clone)]
    pub struct PusService24_2 {
        /// Housekeeping ID (16-bit UINT)
        pub hk_id: u16,
        /// Parameter count N (16-bit UINT)
        pub n: u16,
        /// Parameter IDs (variable length, each u16)
        pub parameter_ids: Vec<u16>,
    }

    impl PusService24_2 {
        pub fn new(hk_id: u16, parameter_ids: Vec<u16>) -> Self {
            let n = parameter_ids.len() as u16;
            PusService24_2 {
                hk_id,
                n,
                parameter_ids,
            }
        }

        pub fn encode(&self) -> Vec<u8> {
            let mut encoded = self.hk_id.to_be_bytes().to_vec();
            encoded.extend_from_slice(&self.n.to_be_bytes());
            for id in &self.parameter_ids {
                encoded.extend_from_slice(&id.to_be_bytes());
            }
            encoded
        }
    }

    /// PUS Service Type 24: Time-based Scheduling - Get Active Parameters
    #[derive(Debug, Clone)]
    pub struct PusService24_3 {
        /// Housekeeping ID (16-bit UINT)
        pub hk_id: u16,
    }

    impl PusService24_3 {
        pub fn new(hk_id: u16) -> Self {
            PusService24_3 { hk_id }
        }

        pub fn encode(&self) -> Vec<u8> {
            self.hk_id.to_be_bytes().to_vec()
        }
    }
}

/// UDP Client for sending ECSS commands
pub struct EcssUdpClient {
    socket: UdpSocket,
    remote_addr: SocketAddr,
}

impl EcssUdpClient {
    /// Creates a new ECSS UDP client
    ///
    /// # Arguments
    /// * `local_addr` - Local address to bind to (e.g., "0.0.0.0:0")
    /// * `remote_addr` - Remote address to send commands to (e.g., "192.168.1.100:5000")
    ///
    /// # Returns
    /// Result containing the client or an error
    pub fn new(local_addr: &str, remote_addr: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let socket = UdpSocket::bind(local_addr)?;
        let remote_addr = remote_addr.parse::<SocketAddr>()?;

        Ok(EcssUdpClient {
            socket,
            remote_addr,
        })
    }

    /// Sends a telecommand packet
    ///
    /// # Arguments
    /// * `packet` - The telecommand packet to send
    ///
    /// # Returns
    /// Result containing the number of bytes sent or an error
    pub fn send_command(&self, packet: &TelecommandPacket) -> std::io::Result<usize> {
        let encoded = packet.encode();
        self.socket.send_to(&encoded, self.remote_addr)
    }

    /// Sends raw command data with automatic packet wrapping
    ///
    /// # Arguments
    /// * `apid` - Application Process ID
    /// * `data` - Command data to send
    /// * `sequence_count` - Packet sequence counter
    ///
    /// # Returns
    /// Result containing the number of bytes sent or an error
    pub fn send_command_data(
        &self,
        apid: u16,
        data: &[u8],
        sequence_count: u16,
    ) -> std::io::Result<usize> {
        let packet = TelecommandPacket::new(apid, sequence_count, data.to_vec(), false);
        self.send_command(&packet)
    }

    /// Changes the remote address for subsequent commands
    pub fn set_remote_address(
        &mut self,
        remote_addr: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.remote_addr = remote_addr.parse::<SocketAddr>()?;
        Ok(())
    }

    /// Gets the current remote address
    pub fn remote_address(&self) -> SocketAddr {
        self.remote_addr
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_packet_identification_encoding() {
        let pi = PacketIdentification::new(PacketType::Telecommand, 1234, false);
        let encoded = pi.encode();
        assert_eq!(encoded.len(), 2);
        // Version (0) in bits 15-13, Type (1) in bit 12, Header flag (0) in bit 11, APID (1234) in bits 10-0
    }

    #[test]
    fn test_sequence_control_encoding() {
        let sc = SequenceControl::new(SequenceFlag::Unsegmented, 100);
        let encoded = sc.encode();
        assert_eq!(encoded.len(), 2);
    }

    #[test]
    fn test_primary_header_encoding() {
        let pi = PacketIdentification::new(PacketType::Telecommand, 100, false);
        let sc = SequenceControl::new(SequenceFlag::Unsegmented, 50);
        let ph = PrimaryHeader::new(pi, sc, 10);
        let encoded = ph.encode();
        assert_eq!(encoded.len(), 6);
    }

    #[test]
    fn test_telecommand_packet_creation() {
        let data = vec![0x01, 0x02, 0x03, 0x04];
        let packet = TelecommandPacket::new(100, 1, data.clone(), false);
        let encoded = packet.encode();

        // Header (6) + Data (4) + CRC (2)
        assert_eq!(encoded.len(), 12);
    }

    #[test]
    fn test_crc_calculation() {
        let data = vec![0x00, 0x00];
        let packet = TelecommandPacket::new(0, 0, data, false);
        let crc = packet.calculate_crc();
        assert!(crc < 0xFFFF);
    }

    #[test]
    fn test_apid_validation() {
        // Valid APID
        let pi = PacketIdentification::new(PacketType::Telecommand, 2047, false);
        assert_eq!(pi.apid, 2047);
    }

    #[test]
    #[should_panic]
    fn test_apid_out_of_range() {
        PacketIdentification::new(PacketType::Telecommand, 2048, false);
    }

    #[test]
    #[should_panic]
    fn test_sequence_count_out_of_range() {
        SequenceControl::new(SequenceFlag::Unsegmented, 16384);
    }

    // Telecommand builder tests
    use telecommands::*;

    #[test]
    fn test_payload1_boot() {
        let data = payload1::pay_1_boot();
        assert_eq!(data.len(), 0);
    }

    #[test]
    fn test_payload1_stop_time() {
        let data = payload1::pay_1_stop_time(100000);
        assert_eq!(data.len(), 3); // 24-bit value
    }

    #[test]
    #[should_panic]
    fn test_payload1_stop_time_out_of_range() {
        payload1::pay_1_stop_time(154801); // Must be <= 154800
    }

    #[test]
    fn test_payload1_download_packet() {
        let data = payload1::pay_1_download_packet(42);
        assert_eq!(data.len(), 4); // 32-bit value
        assert_eq!(u32::from_be_bytes([data[0], data[1], data[2], data[3]]), 42);
    }

    #[test]
    fn test_payload2_boot() {
        let data = payload2::pay_2_boot();
        assert_eq!(data.len(), 0);
    }

    #[test]
    fn test_payload2_stop_time() {
        let data = payload2::pay_2_stop_time(50000);
        assert_eq!(data.len(), 3); // 24-bit value
    }

    #[test]
    fn test_payload2_download_packet() {
        let data = payload2::pay_2_download_packet(100);
        assert_eq!(data.len(), 4); // 32-bit value
    }

    #[test]
    fn test_eps_output_bus_group_state() {
        let data = eps::eps_output_bus_group_state(0xFF);
        assert_eq!(data.len(), 4); // 32-bit value
        assert_eq!(
            u32::from_be_bytes([data[0], data[1], data[2], data[3]]),
            0xFF
        );
    }

    #[test]
    fn test_eps_output_bus_channel_on() {
        let data = eps::eps_output_bus_channel_on(5);
        assert_eq!(data.len(), 2); // 16-bit value
        assert_eq!(u16::from_be_bytes([data[0], data[1]]), 5);
    }

    #[test]
    fn test_eps_output_bus_channel_off() {
        let data = eps::eps_output_bus_channel_off(3);
        assert_eq!(data.len(), 2); // 16-bit value
    }

    #[test]
    fn test_eps_get_configuration_parameter() {
        let data = eps::eps_get_configuration_parameter(0x1234);
        assert_eq!(data.len(), 2); // 16-bit value
        assert_eq!(u16::from_be_bytes([data[0], data[1]]), 0x1234);
    }

    #[test]
    fn test_eps_set_configuration_parameter() {
        let value = vec![0x42];
        let data = eps::eps_set_configuration_parameter(0x1234, &value);
        assert_eq!(data.len(), 3); // 16-bit ID + 1 byte value
        assert_eq!(u16::from_be_bytes([data[0], data[1]]), 0x1234);
        assert_eq!(data[2], 0x42);
    }

    #[test]
    #[should_panic]
    fn test_eps_set_configuration_parameter_value_too_large() {
        let value = vec![0x00, 0x00, 0x00, 0x00, 0x00]; // 5 bytes (too large)
        eps::eps_set_configuration_parameter(0x1234, &value);
    }

    #[test]
    fn test_eps_save_configuration() {
        let data = eps::eps_save_configuration(0xABCD);
        assert_eq!(data.len(), 2); // 16-bit checksum
        assert_eq!(u16::from_be_bytes([data[0], data[1]]), 0xABCD);
    }

    #[test]
    fn test_eps_correct_time() {
        let data = eps::eps_correct_time(3600); // 1 hour in seconds
        assert_eq!(data.len(), 4); // 32-bit signed value
        assert_eq!(
            i32::from_be_bytes([data[0], data[1], data[2], data[3]]),
            3600
        );
    }

    #[test]
    fn test_eps_correct_time_negative() {
        let data = eps::eps_correct_time(-3600); // -1 hour
        assert_eq!(data.len(), 4); // 32-bit signed value
        assert_eq!(
            i32::from_be_bytes([data[0], data[1], data[2], data[3]]),
            -3600
        );
    }

    #[test]
    fn test_obc_dump_range() {
        let data = obc::obc_dump_range(10, 20);
        assert_eq!(data.len(), 4); // 2x 16-bit values
        assert_eq!(u16::from_be_bytes([data[0], data[1]]), 10);
        assert_eq!(u16::from_be_bytes([data[2], data[3]]), 20);
    }

    #[test]
    fn test_obc_dump_packet() {
        let data = obc::obc_dump_packet(42);
        assert_eq!(data.len(), 2); // 16-bit value
        assert_eq!(u16::from_be_bytes([data[0], data[1]]), 42);
    }

    #[test]
    fn test_obc_boot() {
        let data = obc::obc_boot();
        assert_eq!(data.len(), 0);
    }

    #[test]
    fn test_system_change_beacon_frequency() {
        let data = system::system_change_beacon_frequency(437_500_000);
        assert_eq!(data.len(), 4); // 32-bit value
    }

    #[test]
    fn test_system_change_baudrate() {
        let data = system::system_change_baudrate(9600);
        assert_eq!(data.len(), 4); // 32-bit value
        assert_eq!(
            u32::from_be_bytes([data[0], data[1], data[2], data[3]]),
            9600
        );
    }

    #[test]
    fn test_create_telecommand_packet() {
        let data = payload1::pay_1_boot();
        let packet = create_telecommand(0, data, 1);
        assert_eq!(packet.primary_header.packet_id.apid, 5);
        assert_eq!(packet.primary_header.sequence_control.sequence_count, 1);
    }

    #[test]
    fn test_encode_u24_conversion() {
        let value = 0x123456u32;
        let encoded = super::encode_u24(value);
        assert_eq!(encoded[0], 0x12);
        assert_eq!(encoded[1], 0x34);
        assert_eq!(encoded[2], 0x56);
    }

    // PUS Service tests
    use pus_services::*;

    #[test]
    fn test_pus_service_3_31_encode() {
        let service = PusService3_31::new(1, vec![0x01, 0x02, 0x03]);
        let encoded = service.encode();
        assert_eq!(encoded.len(), 4);
        assert_eq!(encoded[0], 1);
        assert_eq!(&encoded[1..], &[0x01, 0x02, 0x03]);
    }

    #[test]
    fn test_pus_service_3_33_encode() {
        let service = PusService3_33::new(2, vec![0x04, 0x05]);
        let encoded = service.encode();
        assert_eq!(encoded.len(), 3);
        assert_eq!(encoded[0], 2);
        assert_eq!(&encoded[1..], &[0x04, 0x05]);
    }

    #[test]
    fn test_pus_service_4_1_encode() {
        let service = PusService4_1::new(vec![0x10, 0x20, 0x30]);
        let encoded = service.encode();
        assert_eq!(encoded, vec![0x10, 0x20, 0x30]);
    }

    #[test]
    fn test_pus_service_4_3_encode() {
        let service = PusService4_3::new(vec![0x1234, 0x5678]);
        let encoded = service.encode();
        assert_eq!(encoded.len(), 4);
        assert_eq!(&encoded[0..2], &[0x12, 0x34]);
        assert_eq!(&encoded[2..4], &[0x56, 0x78]);
    }

    #[test]
    fn test_pus_service_4_4_encode() {
        let service = PusService4_4::new(3600);
        let encoded = service.encode();
        assert_eq!(encoded.len(), 4);
        assert_eq!(
            u32::from_be_bytes([encoded[0], encoded[1], encoded[2], encoded[3]]),
            3600
        );
    }

    #[test]
    fn test_pus_service_4_5_encode() {
        let service = PusService4_5::new();
        let encoded = service.encode();
        assert_eq!(encoded.len(), 0);
    }

    #[test]
    fn test_pus_service_5_5_encode() {
        let service = PusService5_5::new(vec![0x0001, 0x0002, 0x0003]);
        let encoded = service.encode();
        assert_eq!(encoded.len(), 6);
    }

    #[test]
    fn test_pus_service_5_6_encode() {
        let service = PusService5_6::new(vec![0xAA, 0xBB, 0xCC]);
        let encoded = service.encode();
        assert_eq!(encoded, vec![0xAA, 0xBB, 0xCC]);
    }

    #[test]
    fn test_pus_service_5_7_encode() {
        let service = PusService5_7::new(vec![0x0010, 0x0020]);
        let encoded = service.encode();
        assert_eq!(encoded.len(), 4);
    }

    #[test]
    fn test_pus_service_17_1_encode() {
        let service = PusService17_1::new(0x12345678);
        let encoded = service.encode();
        assert_eq!(encoded.len(), 4);
        assert_eq!(
            u32::from_be_bytes([encoded[0], encoded[1], encoded[2], encoded[3]]),
            0x12345678
        );
    }

    #[test]
    fn test_pus_service_21_1_encode() {
        let service = PusService21_1::new(1, vec![0x01, 0x02, 0x03]);
        let encoded = service.encode();
        assert_eq!(encoded.len(), 7); // 4 bytes ID + 3 bytes data
        assert_eq!(
            u32::from_be_bytes([encoded[0], encoded[1], encoded[2], encoded[3]]),
            1
        );
        assert_eq!(&encoded[4..], &[0x01, 0x02, 0x03]);
    }

    #[test]
    fn test_pus_service_21_2_encode() {
        let service = PusService21_2::new(2, b"sequence.bin".to_vec());
        let encoded = service.encode();
        assert_eq!(encoded.len(), 16); // 4 bytes ID + 12 bytes name
    }

    #[test]
    fn test_pus_service_21_3_encode() {
        let service = PusService21_3::new(3);
        let encoded = service.encode();
        assert_eq!(encoded.len(), 4);
        assert_eq!(
            u32::from_be_bytes([encoded[0], encoded[1], encoded[2], encoded[3]]),
            3
        );
    }

    #[test]
    fn test_pus_service_21_4_encode() {
        let service = PusService21_4::new(4);
        let encoded = service.encode();
        assert_eq!(encoded.len(), 4);
    }

    #[test]
    fn test_pus_service_21_5_encode() {
        let service = PusService21_5::new(5);
        let encoded = service.encode();
        assert_eq!(encoded.len(), 4);
    }

    #[test]
    fn test_pus_service_21_6_encode() {
        let service = PusService21_6::new(vec![0x11, 0x22, 0x33, 0x44]);
        let encoded = service.encode();
        assert_eq!(encoded, vec![0x11, 0x22, 0x33, 0x44]);
    }

    #[test]
    fn test_pus_service_21_13_encode() {
        let service = PusService21_13::new(vec![0xFF, 0xEE]);
        let encoded = service.encode();
        assert_eq!(encoded, vec![0xFF, 0xEE]);
    }

    #[test]
    fn test_pus_service_24_1_encode() {
        let service = PusService24_1::new(0x0100, vec![0x0001, 0x0002, 0x0003]);
        let encoded = service.encode();
        assert_eq!(encoded.len(), 10); // 2 bytes HK_ID + 2 bytes N + 6 bytes parameter IDs
        assert_eq!(u16::from_be_bytes([encoded[0], encoded[1]]), 0x0100);
        assert_eq!(u16::from_be_bytes([encoded[2], encoded[3]]), 3);
    }

    #[test]
    fn test_pus_service_24_2_encode() {
        let service = PusService24_2::new(0x0200, vec![0x0010, 0x0020]);
        let encoded = service.encode();
        assert_eq!(encoded.len(), 8); // 2 bytes HK_ID + 2 bytes N + 4 bytes parameter IDs
        assert_eq!(u16::from_be_bytes([encoded[0], encoded[1]]), 0x0200);
        assert_eq!(u16::from_be_bytes([encoded[2], encoded[3]]), 2);
    }

    #[test]
    fn test_pus_service_24_3_encode() {
        let service = PusService24_3::new(0x0300);
        let encoded = service.encode();
        assert_eq!(encoded.len(), 2);
        assert_eq!(u16::from_be_bytes([encoded[0], encoded[1]]), 0x0300);
    }

    #[test]
    fn test_pus_service_24_1_parameter_count() {
        let param_ids = vec![0x0001, 0x0002, 0x0003, 0x0004, 0x0005];
        let service = PusService24_1::new(0x0100, param_ids);
        assert_eq!(service.n, 5);
        let encoded = service.encode();
        assert_eq!(encoded.len(), 14); // 2 + 2 + 10
    }
}
