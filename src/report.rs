use embassy_time::{Duration, Timer};
use log::info;
use serde_json::json;

use crate::global;
use crate::net;
use crate::nvs;

async fn status_report() -> anyhow::Result<String> {
    let device_id = nvs::get_device_id()?;
    let uptime = global::get_uptime();

    let report_json = json!({
        "device_id": device_id,
        "uptime": uptime,
    });

    info!("Report: {}", report_json);
    Ok(report_json.to_string())
}

pub async fn report_loop() -> anyhow::Result<()> {
    let mut wait_secs;
    let mut lauch_report = false;
    loop {
        {
            match global::TIME_SYNCED.try_lock() {
                Ok(time_synced) => {
                    if !*time_synced {
                        wait_secs = 10;
                    } else {
                        lauch_report = true;
                        wait_secs = 60 * 60 * 24;
                    }
                }
                _ => {
                    info!("TIME_SYNCED in use");
                    wait_secs = 1;
                }
            }
        }

        if lauch_report {
            match global::CMD_NET.try_lock() {
                Ok(mut cmd_net) => {
                    *cmd_net = "REPORT".to_string();
                    info!("REPORT cmd sent");
                    lauch_report = false;
                    wait_secs = 60 * 60 * 24;
                }
                Err(_) => {
                    warn!("CMD_NET in use");
                    wait_secs = 1;
                }
            }
        }
    }
    Timer::after(Duration::from_secs(wait_secs)).await;
}
