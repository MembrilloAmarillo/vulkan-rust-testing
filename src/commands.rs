use crate::error::Result;
use crate::xtce_types::{
    ensure_count_matches, hk_array_value, pus331_array_value, u16_array_value, u24, HkStructureId,
    Pus331Entry,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreparedCommand {
    pub name: String,
    pub args: Value,
    pub description: Option<String>,
}

impl PreparedCommand {
    pub fn new(name: impl Into<String>, args: Value) -> Self {
        Self {
            name: name.into(),
            args,
            description: None,
        }
    }

    pub fn with_description(mut self, text: impl Into<String>) -> Self {
        self.description = Some(text.into());
        self
    }
}

pub struct Commands;

impl Commands {
    pub fn pus_17_1() -> PreparedCommand {
        PreparedCommand::new("PUS_17_1", json!({})).with_description("Are-you-alive request")
    }

    pub fn pus_8_1(function_id: u8) -> PreparedCommand {
        PreparedCommand::new(
            "PUS_8_1",
            json!({
                "CCSDS_Source_ID" : 10,
                //"CCSDS_Packet_Length" : 0, // should be calculed given the headers and actual command data
                "Function_ID": function_id,
            }),
        )
        .with_description("Generic PUS 8-1 command")
    }

    pub fn pus_8_1_eps_output_bus_channel_on(channel_id: u8) -> PreparedCommand {
        PreparedCommand::new(
            "PUS_8_1_EPS_OUTPUT_BUS_CHANNEL_ON",
            json!({
                "CCSDS_Source_ID" : 10,
                //"CCSDS_Packet_Length" : 0, // should be calculed given the headers and actual command data
                // this are the PUS[8, 1] specific arguments
                //"Function_ID": "UCF_EPS_OUTPUT_BUS_CHANNEL_ON_ID",
                "Channel_ID": channel_id,
            }),
        )
        .with_description("Turn on EPS output bus channel")
    }

    pub fn pus_8_1_eps_output_bus_channel_off(channel_id: u8) -> PreparedCommand {
        PreparedCommand::new(
            "PUS_8_1_EPS_OUTPUT_BUS_CHANNEL_OFF",
            json!({ "Channel_ID": channel_id }),
        )
        .with_description("Turn off EPS output bus channel")
    }

    pub fn pus_8_1_system_change_time(unix_time: u32) -> PreparedCommand {
        PreparedCommand::new("PUS_8_1_SYSTEM_CHANGE_TIME", json!({ "Time": unix_time }))
            .with_description("Update spacecraft UNIX time")
    }

    pub fn pus_8_1_eps_correct_time(delta_seconds: i32) -> PreparedCommand {
        PreparedCommand::new("PUS_8_1_EPS_CORRECT_TIME", json!({ "Time": delta_seconds }))
            .with_description("Correct EPS time")
    }

    pub fn pus_8_1_pay_1_stop_time_id(stop_time: u32) -> Result<PreparedCommand> {
        Ok(PreparedCommand::new(
            "PUS_8_1_PAY1_STOP_TIME_ID",
            json!({ "Stop_Time": u24(stop_time)? }),
        )
        .with_description("Set payload 1 stop time"))
    }

    pub fn pus_8_1_pay_2_stop_time_id(stop_time: u32) -> Result<PreparedCommand> {
        Ok(PreparedCommand::new(
            "PUS_8_1_PAY2_STOP_TIME_ID",
            json!({ "Stop_Time": u24(stop_time)? }),
        )
        .with_description("Set payload 2 stop time"))
    }

    pub fn pus_8_1_pay_1_download_exp(packet_id: u32) -> PreparedCommand {
        PreparedCommand::new(
            "PUS_8_1_PAY1_DOWNLOAD_EXP",
            json!({ "PacketID": packet_id }),
        )
        .with_description("Download payload 1 experiment packet")
    }

    pub fn pus_8_1_pay_2_download_exp(packet_id: u32) -> PreparedCommand {
        PreparedCommand::new(
            "PUS_8_1_PAY2_DOWNLOAD_EXP",
            json!({ "PacketID": packet_id }),
        )
        .with_description("Download payload 2 experiment packet")
    }

    pub fn pus_8_1_end_of_mission() -> PreparedCommand {
        PreparedCommand::new("PUS_8_1_END_OF_MISSION", json!({})).with_description("End of mission")
    }

    pub fn pus_8_1_end_of_mission_2(decrypted_val: u64) -> PreparedCommand {
        PreparedCommand::new(
            "PUS_8_1_END_OF_MISSION_2",
            json!({ "DecryptedVal": decrypted_val }),
        )
        .with_description("End of mission 2 with decryption")
    }

    pub fn pus_8_1_end_of_mission_3(decrypted_val: u64) -> PreparedCommand {
        PreparedCommand::new(
            "PUS_8_1_END_OF_MISSION_3",
            json!({ "DecryptedVal": decrypted_val }),
        )
        .with_description("End of mission 3 with decryption")
    }

    pub fn pus_3_31(entries: Vec<Pus331Entry>) -> Result<PreparedCommand> {
        let n = entries.len();
        ensure_count_matches(n, &entries, "PUS331Body")?;

        Ok(PreparedCommand::new(
            "PUS_3_31",
            json!({
                "N": n,
                "PUS_3_31_Body": pus331_array_value(&entries),
            }),
        )
        .with_description("Change HK collection intervals"))
    }

    pub fn pus_3_33(hk_ids: Vec<HkStructureId>) -> Result<PreparedCommand> {
        let n = hk_ids.len();
        ensure_count_matches(n, &hk_ids, "HK_Structure_ID")?;

        Ok(PreparedCommand::new(
            "PUS_3_33",
            json!({
                "N": n,
                "HK_Structure_ID": hk_array_value(&hk_ids),
            }),
        )
        .with_description("Query HK collection interval information"))
    }

    pub fn pus_5_5(event_ids: Vec<u16>) -> Result<PreparedCommand> {
        let n = event_ids.len();
        ensure_count_matches(n, &event_ids, "EventID")?;

        Ok(PreparedCommand::new(
            "PUS_5_5",
            json!({
                "N": n,
                "Event_ID": u16_array_value(&event_ids),
            }),
        )
        .with_description("Enable event reports"))
    }

    pub fn pus_5_6(event_ids: Vec<u16>) -> Result<PreparedCommand> {
        let n = event_ids.len();
        ensure_count_matches(n, &event_ids, "EventID")?;

        Ok(PreparedCommand::new(
            "PUS_5_6",
            json!({
                "N": n,
                "Event_ID": u16_array_value(&event_ids),
            }),
        )
        .with_description("Disable event reports"))
    }

    pub fn pus_24_1(hk: HkStructureId, parameter_ids: Vec<u16>) -> Result<PreparedCommand> {
        let n = parameter_ids.len();
        ensure_count_matches(n, &parameter_ids, "ParameterID")?;

        Ok(PreparedCommand::new(
            "PUS_24_1",
            json!({
                "HK_Structure_ID": hk.as_u16(),
                "N": n,
                "Parameter_ID": u16_array_value(&parameter_ids),
            }),
        )
        .with_description("Enable parameter collection for HK structure"))
    }

    pub fn pus_24_2(hk: HkStructureId, parameter_ids: Vec<u16>) -> Result<PreparedCommand> {
        let n = parameter_ids.len();
        ensure_count_matches(n, &parameter_ids, "ParameterID")?;

        Ok(PreparedCommand::new(
            "PUS_24_2",
            json!({
                "HK_Structure_ID": hk.as_u16(),
                "N": n,
                "Parameter_ID": u16_array_value(&parameter_ids),
            }),
        )
        .with_description("Disable parameter collection for HK structure"))
    }

    pub fn pus_24_3(hk: HkStructureId, parameter_ids: Vec<u16>) -> Result<PreparedCommand> {
        let n = parameter_ids.len();
        ensure_count_matches(n, &parameter_ids, "ParameterID")?;

        Ok(PreparedCommand::new(
            "PUS_24_3",
            json!({
                "HK_Structure_ID": hk.as_u16(),
                "N": n,
                "Parameter_ID": u16_array_value(&parameter_ids),
            }),
        )
        .with_description("Query active parameters for HK structure"))
    }
}
