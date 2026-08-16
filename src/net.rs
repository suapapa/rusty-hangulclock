use std::time::Duration as StdDuration;

use embedded_svc::http::client::Client;
use embedded_svc::http::Method;
use embedded_svc::wifi::{
    AccessPointConfiguration, AuthMethod, ClientConfiguration, Configuration,
};
use esp_idf_svc::hal::sys::esp_wifi_set_max_tx_power;
use esp_idf_svc::http::client::{Configuration as HttpConfiguration, EspHttpConnection};
use esp_idf_svc::sntp;
use esp_idf_svc::wifi::{AsyncWifi, EspWifi, WpsConfig, WpsFactoryInfo, WpsStatus, WpsType};
use log::{info, warn};

use crate::{global, nvs, ota_update, report, timer, web_server};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetCmd {
    None,
    Ap,
    Wps,
    Ntp,
    Ota,
}

impl NetCmd {
    fn from_str(s: &str) -> Self {
        match s {
            "AP" => NetCmd::Ap,
            "WPS" => NetCmd::Wps,
            "NTP" => NetCmd::Ntp,
            "OTA" => NetCmd::Ota,
            _ => NetCmd::None,
        }
    }

    // fn as_str(&self) -> &'static str {
    //     match self {
    //         NetCmd::Ap => "AP",
    //         NetCmd::Wps => "WPS",
    //         NetCmd::Ntp => "NTP",
    //         NetCmd::Ota => "OTA",
    //         NetCmd::None => "",
    //     }
    // }
}

pub async fn net_loop(wifi: &mut AsyncWifi<EspWifi<'static>>) -> anyhow::Result<()> {
    info!("Starting net_loop()...");

    // Trigger initial time sync
    let _ = set_net_cmd("NTP");

    let mut watchdog = global::WatchdogManager::new(global::TaskId::Net, 100, 10);

    loop {
        timer::sleep_millis(100).await;

        if watchdog.update() {
            global::yield_to_other_tasks().await;
        }

        let cmd = match get_net_cmd() {
            Ok(s) => NetCmd::from_str(&s),
            Err(e) => {
                warn!("Failed to get net cmd: {}", e);
                continue;
            }
        };

        match cmd {
            NetCmd::Ap => {
                info!("Handling AP command");
                let _stall = NetStallGuard::new(90_000);
                set_result_net("");
                match connect_ap(wifi).await {
                    Ok(_) => {
                        info!("AP command successful");
                        set_result_net("OK");
                        if let Ok(mut mode) = global::AP_MODE.try_lock() {
                            *mode = true;
                        }
                    }
                    Err(e) => {
                        warn!("AP command failed: {:?}", e);
                        set_result_net("NG");
                    }
                }
                clear_net_cmd();
            }
            NetCmd::Wps => {
                info!("Handling WPS command");
                let _stall = NetStallGuard::new(150_000);
                set_result_net("");
                match connect_wps(wifi).await {
                    Ok(_) => {
                        info!("WPS command successful");
                        set_result_net("OK");
                    }
                    Err(e) => {
                        warn!("WPS command failed: {:?}", e);
                        set_result_net("NG");
                    }
                }
                clear_net_cmd();
            }
            NetCmd::Ntp => {
                info!("Handling NTP command");
                let _stall = NetStallGuard::new(120_000);
                set_result_net("");
                match embassy_time::with_timeout(
                    embassy_time::Duration::from_secs(180),
                    sync_time_and_send_report(wifi),
                )
                .await
                {
                    Ok(Ok(_)) => {
                        info!("NTP sync and report successful");
                        set_result_net("OK");
                    }
                    Ok(Err(e)) => {
                        warn!("NTP command failed: {:?}", e);
                        set_result_net("NG");
                    }
                    Err(_) => {
                        warn!("NTP command timed out after 180s");
                        set_result_net("NG");
                    }
                }
                clear_net_cmd();
            }
            NetCmd::Ota => {
                info!("Handling OTA command");
                // No wall-clock timeout: firmware download can take several
                // minutes. Liveness is progress heartbeats (chunk reads).
                let _stall = NetStallGuard::new(120_000);
                if let Ok(mut mode) = global::OTA_MODE.try_lock() {
                    *mode = true;
                }
                set_result_net("");
                match ota_update_with_wifi(wifi).await {
                    Ok(_) => {
                        info!("OTA update successful");
                        set_result_net("OK");
                    }
                    Err(e) => {
                        warn!("OTA update failed: {:?}", e);
                        set_result_net("NG");
                    }
                }
                if let Ok(mut mode) = global::OTA_MODE.try_lock() {
                    *mode = false;
                }
                clear_net_cmd();
            }
            NetCmd::None => {}
        }
    }
}

/// Raises the net_loop stall budget for one command, then restores it.
struct NetStallGuard;

impl NetStallGuard {
    fn new(limit_ms: u32) -> Self {
        global::heartbeat(global::TaskId::Net);
        global::set_net_stall_limit_ms(limit_ms);
        Self
    }
}

impl Drop for NetStallGuard {
    fn drop(&mut self) {
        global::reset_net_stall_limit();
        global::heartbeat(global::TaskId::Net);
    }
}

fn apply_wifi_stability_settings() {
    unsafe {
        esp_wifi_set_max_tx_power(34);
        esp_idf_svc::hal::sys::esp_wifi_set_ps(esp_idf_svc::hal::sys::wifi_ps_type_t_WIFI_PS_NONE);
    }
}

pub async fn connect_ap(wifi: &mut AsyncWifi<EspWifi<'static>>) -> anyhow::Result<()> {
    let device_no = nvs::get_device_no().unwrap_or_else(|_| "0000".to_string());
    let ssid = format!("rusty-hangulclock-{}", device_no);

    let config = Configuration::AccessPoint(AccessPointConfiguration {
        ssid: ssid.as_str().try_into().unwrap(),
        password: "12345678".try_into().unwrap(),
        max_connections: 1,
        auth_method: AuthMethod::WPA2Personal,
        ..Default::default()
    });

    wifi.set_configuration(&config)?;
    embassy_time::with_timeout(embassy_time::Duration::from_secs(15), wifi.start())
        .await
        .map_err(|_| anyhow::anyhow!("WiFi start timeout (AP)"))??;

    embassy_time::with_timeout(embassy_time::Duration::from_secs(30), wifi.wait_netif_up())
        .await
        .map_err(|_| anyhow::anyhow!("WiFi AP wait netif up timeout"))??;

    web_server::start_web_server().await?;
    Ok(())
}

pub async fn connect_wps(wifi: &mut AsyncWifi<EspWifi<'static>>) -> anyhow::Result<()> {
    let dummy_config = Configuration::Client(ClientConfiguration {
        ssid: "dummy_ssid".try_into().unwrap(),
        password: "dummy_password".try_into().unwrap(),
        ..Default::default()
    });
    wifi.set_configuration(&dummy_config)?;
    embassy_time::with_timeout(embassy_time::Duration::from_secs(15), wifi.start())
        .await
        .map_err(|_| anyhow::anyhow!("WiFi start timeout (WPS)"))??;
    apply_wifi_stability_settings();

    let hw_rev = global::get_hw_revision();
    let device_no = nvs::get_device_no().unwrap_or_else(|_| "0000".to_string());

    let model_number = format!("rhc-{}", hw_rev);
    let model_name = format!("Rusty HangulClock Rev.{}", hw_rev);
    let device_name = format!("rusty-hangulclock-{}", device_no);

    let wps_config = WpsConfig {
        wps_type: WpsType::Pbc,
        factory_info: WpsFactoryInfo {
            manufacturer: "homin.dev",
            model_number: &model_number,
            model_name: &model_name,
            device_name: &device_name,
        },
    };

    let mut retries = 5;
    while retries > 0 {
        global::heartbeat(global::TaskId::Net);
        match embassy_time::with_timeout(
            embassy_time::Duration::from_secs(130),
            wifi.start_wps(&wps_config),
        )
        .await
        {
            Err(_) => {
                warn!("WPS start timed out");
                retries -= 1;
                timer::sleep_secs(3).await;
                continue;
            }
            Ok(Ok(WpsStatus::SuccessConnected)) => break,
            Ok(Ok(WpsStatus::SuccessMultipleAccessPoints(credentials))) => {
                info!("Multiple credentials received, connecting to first");
                let cred = &credentials[0];
                wifi.set_configuration(&Configuration::Client(ClientConfiguration {
                    ssid: cred.ssid.clone(),
                    password: cred.passphrase.clone(),
                    ..Default::default()
                }))?;
                break;
            }
            Ok(Ok(WpsStatus::Failure | WpsStatus::Timeout)) => {
                retries -= 1;
                timer::sleep_secs(3).await;
                continue;
            }
            Ok(Ok(other)) => return Err(anyhow::anyhow!("WPS failed with status: {:?}", other)),
            Ok(Err(e)) => return Err(anyhow::anyhow!("WPS error: {:?}", e)),
        }
    }

    if let Ok(Configuration::Client(config)) = wifi.get_configuration() {
        nvs::set_wifi_cred(&config.ssid, &config.password)?;
    }

    connect_wifi_with_timeout(wifi).await?;
    let _ = sync_time_without_wifi().await;
    let _ = send_report_without_wifi().await;

    embassy_time::with_timeout(embassy_time::Duration::from_secs(15), wifi.stop())
        .await
        .map_err(|_| anyhow::anyhow!("WiFi stop timeout (WPS)"))??;
    Ok(())
}

async fn sync_time_without_wifi() -> anyhow::Result<()> {
    let sntp_conf = sntp::SntpConf {
        servers: ["time.google.com"],
        operating_mode: sntp::OperatingMode::Poll,
        sync_mode: sntp::SyncMode::Immediate,
    };

    let sntp = sntp::EspSntp::new(&sntp_conf)?;
    let mut retries = 5;

    while retries > 0 {
        match sntp.get_sync_status() {
            sntp::SyncStatus::Completed => {
                info!("SNTP sync completed");
                if let Ok(mut synced) = global::TIME_SYNCED.try_lock() {
                    *synced = true;
                }
                return Ok(());
            }
            _ => {
                retries -= 1;
                global::heartbeat(global::TaskId::Net);
                timer::sleep_secs(5).await;
            }
        }
    }
    Err(anyhow::anyhow!("SNTP sync timed out"))
}

async fn send_report_without_wifi() -> anyhow::Result<()> {
    for attempt in 1..=3 {
        global::heartbeat(global::TaskId::Net);
        match try_send_report().await {
            Ok(_) => return Ok(()),
            Err(e) => {
                warn!("Report attempt {} failed: {:?}", attempt, e);
                if attempt < 3 {
                    timer::sleep_secs(5).await;
                }
            }
        }
    }
    Err(anyhow::anyhow!("Failed to send report after 3 attempts"))
}

async fn try_send_report() -> anyhow::Result<u16> {
    let connection = EspHttpConnection::new(&HttpConfiguration {
        use_global_ca_store: true,
        crt_bundle_attach: Some(esp_idf_svc::sys::esp_crt_bundle_attach),
        timeout: Some(StdDuration::from_secs(30)),
        ..Default::default()
    })?;
    let mut client = Client::wrap(connection);

    let auth_header = format!("Bearer {}", get_api_token());
    let headers = [
        ("Content-Type", "application/json"),
        ("Authorization", &auth_header),
    ];

    let url = "https://hangulclock.homin.dev/v1/live-status";
    let mut request = client.request(Method::Post, url, &headers)?;
    let report_json = report::status_report().await?;

    request.write(report_json.as_bytes())?;
    request.flush()?;

    let response = request.submit()?;
    Ok(response.status())
}

pub async fn sync_time_and_send_report(
    wifi: &mut AsyncWifi<EspWifi<'static>>,
) -> anyhow::Result<()> {
    let (ssid, pass) = nvs::get_wifi_cred()?;

    wifi.set_configuration(&Configuration::Client(ClientConfiguration {
        ssid: ssid.as_str().try_into().unwrap(),
        password: pass.as_str().try_into().unwrap(),
        ..Default::default()
    }))?;

    embassy_time::with_timeout(embassy_time::Duration::from_secs(15), wifi.start())
        .await
        .map_err(|_| anyhow::anyhow!("WiFi start timeout"))??;
    apply_wifi_stability_settings();
    connect_wifi_with_timeout(wifi).await?;

    let _ = sync_time_without_wifi().await;
    let _ = send_report_without_wifi().await;

    embassy_time::with_timeout(embassy_time::Duration::from_secs(15), wifi.stop())
        .await
        .map_err(|_| anyhow::anyhow!("WiFi stop timeout"))??;
    Ok(())
}

async fn ota_update_with_wifi(wifi: &mut AsyncWifi<EspWifi<'static>>) -> anyhow::Result<()> {
    let (ssid, pass) = nvs::get_wifi_cred()?;

    wifi.set_configuration(&Configuration::Client(ClientConfiguration {
        ssid: ssid.as_str().try_into().unwrap(),
        password: pass.as_str().try_into().unwrap(),
        ..Default::default()
    }))?;

    embassy_time::with_timeout(embassy_time::Duration::from_secs(15), wifi.start())
        .await
        .map_err(|_| anyhow::anyhow!("WiFi start timeout (OTA)"))??;
    apply_wifi_stability_settings();
    connect_wifi_with_timeout(wifi).await?;

    let res = ota_update::ota_update().await;
    embassy_time::with_timeout(embassy_time::Duration::from_secs(15), wifi.stop())
        .await
        .map_err(|_| anyhow::anyhow!("WiFi stop timeout (OTA)"))??;
    res
}

pub fn set_net_cmd(cmd: &str) -> bool {
    if let Ok(mut cmd_net) = global::CMD_NET.try_lock() {
        if !cmd_net.is_empty() {
            warn!("Net command already in progress: {}", *cmd_net);
            return false;
        }
        *cmd_net = cmd.to_string();
        true
    } else {
        false
    }
}

pub fn get_net_cmd() -> Result<String, String> {
    global::CMD_NET
        .try_lock()
        .map(|guard| guard.clone())
        .map_err(|_| "CMD_NET lock failed".to_string())
}

fn clear_net_cmd() {
    for _ in 0..100 {
        if let Ok(mut guard) = global::CMD_NET.try_lock() {
            guard.clear();
            return;
        }
        std::thread::yield_now();
    }
    warn!("Failed to clear net cmd after retries");
}

pub fn set_result_net(result: &str) -> bool {
    if let Ok(mut guard) = global::RESULT_NET.lock() {
        *guard = result.to_string();
        true
    } else {
        false
    }
}

pub fn get_result_net() -> String {
    global::RESULT_NET
        .try_lock()
        .map(|guard| guard.clone())
        .unwrap_or_default()
}

pub async fn check_net_cmd_or_skip() -> Result<(), &'static str> {
    if let Ok(cmd) = get_net_cmd() {
        if !cmd.is_empty() {
            return Err("net_cmd_in_progress");
        }
        Ok(())
    } else {
        Err("net_lock_failed")
    }
}

async fn connect_wifi_with_timeout(wifi: &mut AsyncWifi<EspWifi<'static>>) -> anyhow::Result<()> {
    embassy_time::with_timeout(embassy_time::Duration::from_secs(30), wifi.connect())
        .await
        .map_err(|_| anyhow::anyhow!("WiFi connect timeout"))??;

    embassy_time::with_timeout(embassy_time::Duration::from_secs(30), wifi.wait_netif_up())
        .await
        .map_err(|_| anyhow::anyhow!("WiFi wait netif up timeout"))??;

    Ok(())
}

pub const fn get_api_token() -> &'static str {
    match option_env!("RUSTY_HANGULCLOCK_TOKEN") {
        Some(token) => token,
        _ => "0000",
    }
}
