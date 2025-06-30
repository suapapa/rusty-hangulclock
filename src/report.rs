use log::info;
use serde_json::json;

use crate::global;
use crate::nvs;

pub async fn status_report() -> anyhow::Result<String> {
    let device_id = nvs::get_device_id()?;
    let boot_count = nvs::get_boot_count()?;
    let uptime = global::get_uptime();

    let report_json = json!({
        "serial": device_id,
        "uptime": uptime,
        "name": "rusty-hangulclock",
        "hw_revision": global::get_hw_revision(),
        "no": nvs::get_device_no()?,
        "sw_version": global::get_sw_version(),
        "boot_count": boot_count,
    });

    info!("Report: {}", report_json);
    Ok(report_json.to_string())
}
