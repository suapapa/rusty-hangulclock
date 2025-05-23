use embassy_time::{Duration, Timer};
use log::{info, warn};
use serde_json::json;

use crate::global;
use crate::nvs;

pub const fn get_device_no() -> &'static str {
    match option_env!("RUSTY_HANGULCLOCK_NO") {
        Some(s) => s,
        None => "0000",
    }
}

pub async fn status_report() -> anyhow::Result<String> {
    let device_id = nvs::get_device_id()?;
    let uptime = global::get_uptime();

    // {
    //   "name": "rusty-hangulclock",
    //   "no": 1,
    //   "serial": "HC-2024-001",
    //   "uptime": 0
    // }
    let report_json = json!({
        "serial": device_id,
        "uptime": uptime,
        "no": get_device_no(),
        "name": "rusty-hangulclock"
    });

    info!("Report: {}", report_json);
    Ok(report_json.to_string())
}

pub async fn report_loop() -> anyhow::Result<()> {
    let mut wait_secs: u64 = 0;
    let mut launch_report = false;
    loop {
        {
            wait_secs = 1;
            match global::TIME_SYNCED.try_lock() {
                Ok(time_synced) => {
                    launch_report = *time_synced;
                }
                _ => {
                    info!("TIME_SYNCED in use");
                }
            }
        }

        if launch_report {
            match global::CMD_NET.try_lock() {
                Ok(mut cmd_net) => {
                    *cmd_net = "REPORT".to_string();
                    info!("REPORT cmd sent");
                    launch_report = false;
                    wait_secs = 60 * 60 * 24;
                }
                Err(_) => {
                    warn!("CMD_NET in use");
                    wait_secs = 1;
                }
            }
        }
        Timer::after(Duration::from_secs(wait_secs)).await;
    }
}
