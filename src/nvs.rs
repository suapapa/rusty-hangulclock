use esp_idf_svc::nvs::*;
use log::info;

fn get_nvs(namespace: &str, read_only: bool) -> anyhow::Result<EspNvs<NvsCustom>> {
    let partition = EspCustomNvsPartition::take("user_nvs")?;
    EspNvs::new(partition, namespace, !read_only)
        .map_err(|e| anyhow::anyhow!("Failed to open NVS namespace {:?}: {:?}", namespace, e))
}

pub fn set_wifi_cred(ssid: &str, pass: &str) -> anyhow::Result<()> {
    let nvs = get_nvs("cred_ns", false)?;

    nvs.set_str("ssid", ssid)
        .map_err(|e| anyhow::anyhow!("Failed to set ssid: {:?}", e))?;
    nvs.set_str("pass", pass)
        .map_err(|e| anyhow::anyhow!("Failed to set pass: {:?}", e))?;

    info!("WiFi credentials updated");
    Ok(())
}

pub fn get_wifi_cred() -> anyhow::Result<(String, String)> {
    let nvs = get_nvs("cred_ns", true)?;

    const MAX_STR_LEN: usize = 100;
    let mut buffer = [0u8; MAX_STR_LEN];

    let ssid = nvs
        .get_str("ssid", &mut buffer)?
        .ok_or_else(|| anyhow::anyhow!("SSID not found"))?
        .to_string();

    let mut buffer = [0u8; MAX_STR_LEN];
    let pass = nvs
        .get_str("pass", &mut buffer)?
        .ok_or_else(|| anyhow::anyhow!("Password not found"))?
        .to_string();

    Ok((ssid, pass))
}

pub fn set_hsv(hue: u8, sat: u8, val: u8) -> anyhow::Result<()> {
    let nvs = get_nvs("hsv_ns", false)?;

    nvs.set_u8("hue", hue)?;
    nvs.set_u8("sat", sat)?;
    nvs.set_u8("val", val)?;

    info!("HSV values updated: ({}, {}, {})", hue, sat, val);
    Ok(())
}

pub fn get_hsv() -> anyhow::Result<(u8, u8, u8)> {
    let nvs = get_nvs("hsv_ns", true)?;

    let hue = nvs.get_u8("hue")?.unwrap_or(0);
    let sat = nvs.get_u8("sat")?.unwrap_or(255);
    let val = nvs.get_u8("val")?.unwrap_or(255);

    Ok((hue, sat, val))
}

pub fn set_utc_offset(offset: i32) -> anyhow::Result<()> {
    let nvs = get_nvs("utc_offset_ns", false)?;
    nvs.set_i32("offset", offset)?;
    info!("UTC offset updated: {}", offset);
    Ok(())
}

pub fn get_utc_offset() -> anyhow::Result<i32> {
    let nvs = get_nvs("utc_offset_ns", true)?;
    let offset = nvs.get_i32("offset")?.unwrap_or(9); // Default to KST
    Ok(offset)
}

pub fn get_device_id() -> anyhow::Result<String> {
    let nvs = get_nvs("device_id_ns", false)?;

    const MAX_STR_LEN: usize = 100;
    let mut buffer = [0u8; MAX_STR_LEN];

    if let Some(id) = nvs.get_str("device_id", &mut buffer)? {
        return Ok(id.to_string());
    }

    // Generate new ID if not found
    let timestamp = chrono::Utc::now()
        .with_timezone(&chrono::FixedOffset::east_opt(9 * 3600).unwrap())
        .to_rfc3339();
    let random = rand::random::<u32>();
    let new_id = format!("{}-{}", timestamp, random);

    nvs.set_str("device_id", &new_id)?;
    info!("New device ID generated: {}", new_id);
    Ok(new_id)
}

pub fn get_boot_count() -> anyhow::Result<u32> {
    let nvs = get_nvs("boot_count_ns", true)?;
    let count = nvs.get_u32("boot_count")?.unwrap_or(0);
    Ok(count)
}

pub fn set_boot_count(count: u32) -> anyhow::Result<()> {
    let nvs = get_nvs("boot_count_ns", false)?;
    nvs.set_u32("boot_count", count)?;
    Ok(())
}

pub fn get_device_no() -> anyhow::Result<String> {
    let nvs = get_nvs("device_no_ns", false)?;

    const MAX_STR_LEN: usize = 10;
    let mut buffer = [0u8; MAX_STR_LEN];

    if let Some(no) = nvs.get_str("device_no", &mut buffer)? {
        return Ok(no.to_string());
    }

    let env_no = option_env!("RUSTY_HANGULCLOCK_NO").unwrap_or("0000");
    nvs.set_str("device_no", env_no)?;
    Ok(env_no.to_string())
}

pub fn get_owner() -> anyhow::Result<String> {
    let nvs = get_nvs("owner_ns", false)?;

    const MAX_STR_LEN: usize = 100;
    let mut buffer = [0u8; MAX_STR_LEN];

    if let Some(owner) = nvs.get_str("owner", &mut buffer)? {
        return Ok(owner.to_string());
    }

    let env_owner = option_env!("RUSTY_HANGULCLOCK_OWNER").unwrap_or("");
    nvs.set_str("owner", env_owner)?;
    Ok(env_owner.to_string())
}
