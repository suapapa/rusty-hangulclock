use serde_json::json;
use log::{info, warn};

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
    let boot_count = nvs::get_boot_count()?;
    let uptime = global::get_uptime();

    let report_json = json!({
        "serial": device_id,
        "uptime": uptime,
        "no": get_device_no(),
        "name": "rusty-hangulclock",
        "boot_count": boot_count,
    });

    info!("Report: {}", report_json);
    Ok(report_json.to_string())
}