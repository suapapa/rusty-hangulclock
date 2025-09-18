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
use log::{debug, info, warn};

use crate::{global, nvs, ota_update, report, timer, web_server};

pub const fn get_api_token() -> &'static str {
    match option_env!("RUSTY_HANGULCLOCK_TOKEN") {
        Some(s) => s,
        None => "0000",
    }
}

pub async fn net_loop(
    wifi: &mut AsyncWifi<EspWifi<'static>>,
    // mut debug_led: impl embedded_hal::digital::OutputPin,
) -> anyhow::Result<()> {
    // debug_led.set_high().unwrap();
    info!("Starting net_loop()...");

    info!("Triggering initial time sync...");
    if !set_net_cmd("NTP") {
        warn!("Failed to send NTP cmd");
    }

    let mut ap_mode = false;
    let mut ota_mode = false;
    // Watchdog 카운터 추가
    let mut watchdog_counter = 0;
    const WATCHDOG_INTERVAL: u32 = 100; // 100ms * 100 = 10초마다 체크

    // let mut auto_time_sync_check_cnt = 0; // 24 hours 마다 수행
    // const AUTO_TIME_SYNC_CHECK_INTERVAL: u32 = 10 * 60 * 60 * 24; // 24 hours

    loop {
        timer::sleep_millis(100).await;

        if ap_mode || ota_mode {
            continue;
        }

        // Watchdog 체크
        watchdog_counter += 1;
        if watchdog_counter >= WATCHDOG_INTERVAL {
            debug!("Net loop watchdog reset");
            watchdog_counter = 0;
            global::reset_task_watchdog();
        }

        // Yield to other tasks periodically
        if watchdog_counter % 10 == 0 {
            global::yield_to_other_tasks().await;
        }

        // auto_time_sync_check_cnt += 1;
        // if auto_time_sync_check_cnt >= AUTO_TIME_SYNC_CHECK_INTERVAL {
        //     info!("Auto time sync check");
        //     auto_time_sync_check_cnt = 0;
        //     if !set_net_cmd("NTP") {
        //         warn!("Failed to send NTP cmd");
        //     }
        // }

        match get_net_cmd() {
            Ok(cmd) => {
                if cmd == "AP" {
                    info!("Received AP command");
                    set_result_net("");
                    match connect_ap(wifi).await {
                        Ok(_) => {
                            info!("AP cmd completed");
                            set_result_net("OK");
                            ap_mode = true;
                        }
                        Err(e) => {
                            warn!("Failed to connect to wifi with ap: {e:?}");
                            set_result_net("NG");
                        }
                    }
                    clear_net_cmd();
                }
                if cmd == "WPS" {
                    info!("Received WPS command");
                    set_result_net("");
                    match connect_wps(wifi).await {
                        Ok(_) => {
                            info!("WPS cmd completed");
                            set_result_net("OK");
                        }
                        Err(e) => {
                            warn!("Failed to connect to wifi with wps: {e:?}");
                            set_result_net("NG");
                        }
                    }
                    clear_net_cmd();
                }
                if cmd == "NTP" {
                    info!("Received NTP command");
                    set_result_net("");
                    match sync_time_and_send_report(wifi).await {
                        Ok(_) => {
                            info!("Report sent and time synced");
                            set_result_net("OK");
                        }
                        Err(e) => {
                            warn!("Failed to send report: {e:?}");
                            set_result_net("NG");
                        }
                    }
                    clear_net_cmd();
                }
                if cmd == "OTA" {
                    info!("Received OTA command");
                    ota_mode = true;
                    set_result_net("");
                    match ota_update_with_wifi(wifi).await {
                        Ok(_) => {
                            info!("OTA cmd completed");
                            set_result_net("OK");
                        }
                        Err(e) => {
                            warn!("Failed to update: {e:?}");
                            set_result_net("NG");
                        }
                    }
                    clear_net_cmd();
                }
                if cmd.is_empty() {
                    debug!("Received empty command");
                }
            }
            Err(e) => {
                warn!("Failed to get net cmd: {e}");
            }
        }
    }
}

pub async fn connect_ap(wifi: &mut AsyncWifi<EspWifi<'static>>) -> anyhow::Result<()> {
    let device_no = nvs::get_device_no().unwrap_or("0000".to_string());

    let wifi_configuration: Configuration = Configuration::AccessPoint(AccessPointConfiguration {
        ssid: format!("rusty-hangulclock-{device_no}")
            .as_str()
            .try_into()
            .unwrap(),
        password: "12345678".try_into().unwrap(),
        max_connections: 1,
        auth_method: AuthMethod::WPA2Personal,
        ..Default::default()
    });
    wifi.set_configuration(&wifi_configuration)?;

    wifi.start().await?;
    info!("Wifi started");

    wifi.wait_netif_up().await?;
    info!("Wifi netif up");

    web_server::start_web_server().await?;

    Ok(())
}

pub async fn connect_wps(wifi: &mut AsyncWifi<EspWifi<'static>>) -> anyhow::Result<()> {
    // let _write_guard = LED_WRITE_LOCK.write().unwrap();

    let wifi_configuration: Configuration = Configuration::Client(ClientConfiguration {
        ssid: "dummy_ssid".try_into().unwrap(),
        password: "dummy_password".try_into().unwrap(),
        bssid: None,
        auth_method: AuthMethod::WPA2Personal,
        channel: None,
        ..Default::default()
    });
    wifi.set_configuration(&wifi_configuration)?;

    wifi.start().await?;
    info!("Wifi started");

    unsafe { esp_wifi_set_max_tx_power(34) };

    // Additional network stability configurations
    // Set WiFi sleep type to NONE for better connection stability
    unsafe {
        esp_idf_svc::hal::sys::esp_wifi_set_ps(esp_idf_svc::hal::sys::wifi_ps_type_t_WIFI_PS_NONE)
    };

    info!("Starting WPS...");
    let hw_rev = global::get_hw_revision();
    let device_no = nvs::get_device_no().unwrap_or("0000".to_string());
    let model_number = format!("rhc-{hw_rev}");
    let model_name = format!("Rusty HangulClock Rev.{hw_rev}");
    let device_name = format!("rusty-hangulclock-{device_no}");
    let wps_config = WpsConfig {
        wps_type: WpsType::Pbc,
        factory_info: WpsFactoryInfo {
            manufacturer: "homin.dev",
            model_number: model_number.as_str(),
            model_name: model_name.as_str(),
            device_name: device_name.as_str(),
        },
    };

    let mut retryies = 5;
    loop {
        retryies -= 1;
        if retryies == 0 {
            return Err(anyhow::anyhow!("WPS failed"));
        }
        match wifi.start_wps(&wps_config).await {
            Ok(WpsStatus::SuccessConnected) => {
                info!("WPS success connected");
                break;
            }
            Ok(WpsStatus::SuccessMultipleAccessPoints(credentials)) => {
                log::info!("received multiple credentials, connecting to first one:");
                for i in &credentials {
                    log::info!(" - ssid: {}", i.ssid);
                }
                let wifi_configuration: Configuration =
                    Configuration::Client(ClientConfiguration {
                        ssid: credentials[0].ssid.clone(),
                        bssid: None,
                        auth_method: AuthMethod::WPA2Personal,
                        password: credentials[1].passphrase.clone(),
                        channel: None,
                        ..Default::default()
                    });
                wifi.set_configuration(&wifi_configuration)?;
                break;
            }
            Ok(WpsStatus::Failure) => {
                info!("WPS failure");
                global::yield_to_other_tasks().await;
                timer::sleep_secs(3).await;
                continue;
            }
            Ok(WpsStatus::Timeout) => {
                info!("WPS timeout");
                global::yield_to_other_tasks().await;
                timer::sleep_secs(3).await;
                continue;
            }
            Ok(WpsStatus::Pin(_)) => {
                return Err(anyhow::anyhow!("WPS pin"));
            }
            Ok(WpsStatus::PbcOverlap) => {
                return Err(anyhow::anyhow!("WPS PBC overlap"));
            }
            Err(e) => {
                return Err(anyhow::anyhow!("WPS error: {e:?}"));
            }
        }
    }

    match wifi.get_configuration() {
        Ok(Configuration::Client(config)) => {
            info!("Successfully connected to {} using WPS", config.ssid);
            nvs::set_wifi_cred(&config.ssid.clone(), &config.password.clone())?;
        }
        _ => return Err(anyhow::anyhow!("Not in station mode")),
    };

    info!("Starting SNTP and send report...");
    wifi.connect().await?;
    info!("Wifi connected");

    wifi.wait_netif_up().await?;
    info!("Wifi netif up");

    // // Add delay to ensure network is fully initialized
    // timer::sleep_secs(3).await; // Increased from 2s to 3s
    // info!("Network initialization delay completed");

    // // Additional network stability check
    // timer::sleep_millis(500).await;
    // info!("Additional network stability delay completed");

    match sync_time_without_wifi().await {
        Ok(_) => {
            info!("Time synced");
        }
        Err(e) => {
            warn!("Failed to sync time: {e:?}");
        }
    }
    match send_report_without_wifi().await {
        Ok(_) => {
            info!("Report sent");
        }
        Err(e) => {
            warn!("Failed to send report: {e:?}");
        }
    }

    wifi.stop().await?;
    info!("Wifi stopped");

    Ok(())
}

async fn sync_time_without_wifi() -> anyhow::Result<bool> {
    let last_time_synced = {
        let last_time_synced = global::TIME_SYNCED.lock().unwrap();
        *last_time_synced
    };

    let sntp_conf = sntp::SntpConf {
        servers: ["time.google.com"], // "pool.ntp.org"
        operating_mode: sntp::OperatingMode::Poll,
        sync_mode: sntp::SyncMode::Immediate,
    };

    let sntp = sntp::EspSntp::new(&sntp_conf).expect("Failed to create SNTP");
    let mut retry = 5;
    loop {
        retry -= 1;
        if retry == 0 {
            return Err(anyhow::anyhow!("Failed to sync time"));
        }
        info!("Waiting for SNTP sync...");
        match sntp.get_sync_status() {
            sntp::SyncStatus::Completed => {
                info!("SNTP synced");
                if !last_time_synced {
                    info!("Setting initial time_synced");
                    let mut time_synced = global::TIME_SYNCED.lock().unwrap();
                    *time_synced = true;
                }
                return Ok(true);
            }
            sntp::SyncStatus::Reset => {
                warn!("SNTP reset");
                global::yield_to_other_tasks().await;
                timer::sleep_secs(3).await;
                continue;
            }
            sntp::SyncStatus::InProgress => {
                info!("SNTP in progress");
                global::yield_to_other_tasks().await;
                timer::sleep_secs(3).await;
                continue;
            }
        }
    }
}

async fn send_report_without_wifi() -> anyhow::Result<()> {
    const MAX_RETRIES: usize = 3;
    const RETRY_DELAY_MS: u64 = 5000; // 5 seconds between retries

    for attempt in 1..=MAX_RETRIES {
        info!("Attempting to send report (attempt {attempt}/{MAX_RETRIES})");

        match try_send_report().await {
            Ok(status) => {
                info!("Report sent successfully with status: {status}");
                return Ok(());
            }
            Err(e) => {
                warn!("Attempt {attempt} failed: {e:?}");
                if attempt < MAX_RETRIES {
                    info!("Retrying in {} seconds...", RETRY_DELAY_MS / 1000);
                    timer::sleep_millis(RETRY_DELAY_MS).await;
                } else {
                    warn!("All {MAX_RETRIES} attempts failed, giving up");
                    return Err(e);
                }
            }
        }
    }

    unreachable!()
}

pub async fn sync_time_and_send_report(
    wifi: &mut AsyncWifi<EspWifi<'static>>,
) -> anyhow::Result<()> {
    match nvs::get_wifi_cred() {
        Ok((ssid, pass)) => {
            let wifi_configuration: Configuration = Configuration::Client(ClientConfiguration {
                ssid: ssid.as_str().try_into().unwrap(),
                bssid: None,
                auth_method: AuthMethod::WPA2Personal,
                password: pass.as_str().try_into().unwrap(),
                channel: None,
                ..Default::default()
            });

            wifi.set_configuration(&wifi_configuration)?;
        }
        Err(e) => {
            warn!("Failed to load wifi cred: {e:?}");
            return Err(e);
        }
    }

    wifi.start().await?;
    info!("Wifi started");

    unsafe { esp_wifi_set_max_tx_power(34) };

    wifi.connect().await?;
    info!("Wifi connected");

    wifi.wait_netif_up().await?;
    info!("Wifi netif up");

    // // Add delay to ensure network is fully initialized
    // timer::sleep_secs(3).await; // Increased from 2s to 3s
    // info!("Network initialization delay completed");

    // // Additional network stability check
    // timer::sleep_millis(500).await;
    // info!("Additional network stability delay completed");

    match sync_time_without_wifi().await {
        Ok(_) => {
            info!("Time synced");
        }
        Err(e) => {
            warn!("Failed to sync time: {e:?}");
        }
    }
    match send_report_without_wifi().await {
        Ok(_) => {
            info!("Report sent");
        }
        Err(e) => {
            warn!("Failed to send report: {e:?}");
        }
    }

    wifi.stop().await?;
    info!("Wifi stopped");

    Ok(())
}

async fn try_send_report() -> anyhow::Result<u16> {
    let connection = EspHttpConnection::new(&HttpConfiguration {
        use_global_ca_store: true,
        crt_bundle_attach: Some(esp_idf_svc::sys::esp_crt_bundle_attach),
        timeout: Some(StdDuration::from_secs(30)), // Increased from 10s to 30s
        ..Default::default()
    })?;
    let mut client = Client::wrap(connection);

    let auth_header = format!("Bearer {}", get_api_token());
    let headers = [
        ("Content-Type", "application/json"),
        ("Authorization", auth_header.as_str()),
    ];

    let url = "https://hangulclock.homin.dev/v1/live-status";
    info!("Attempting to connect to {url}");
    info!("Before client.request");

    // Add network status check before making request
    info!("Network status check - attempting to create HTTP request");
    let mut request = client.request(Method::Post, url.as_ref(), &headers)?;
    info!("After client.request - HTTP request created successfully");

    info!("Sending report data");
    let report_json = report::status_report().await?;
    request.write(report_json.as_bytes())?;
    request.flush()?;

    info!("Waiting for response");
    info!("Submitting HTTP request - this may take up to 30 seconds...");
    let response = request.submit()?;
    let status = response.status();

    info!("Response received successfully with status: {status}");

    Ok(status)
}

async fn ota_update_with_wifi(wifi: &mut AsyncWifi<EspWifi<'static>>) -> anyhow::Result<()> {
    match nvs::get_wifi_cred() {
        Ok((ssid, pass)) => {
            let wifi_configuration: Configuration = Configuration::Client(ClientConfiguration {
                ssid: ssid.as_str().try_into().unwrap(),
                bssid: None,
                auth_method: AuthMethod::WPA2Personal,
                password: pass.as_str().try_into().unwrap(),
                channel: None,
                ..Default::default()
            });

            wifi.set_configuration(&wifi_configuration)?;
        }
        Err(e) => {
            warn!("Failed to load wifi cred: {e:?}");
            return Err(e);
        }
    }

    wifi.start().await?;
    info!("Wifi started");

    unsafe { esp_wifi_set_max_tx_power(34) };

    wifi.connect().await?;
    info!("Wifi connected");

    wifi.wait_netif_up().await?;
    info!("Wifi netif up");

    // // Add delay to ensure network is fully initialized
    // timer::sleep_secs(3).await; // Increased from 2s to 3s
    // info!("Network initialization delay completed");

    // // Additional network stability check
    // timer::sleep_millis(500).await;
    // info!("Additional network stability delay completed");

    let ota_result = ota_update::ota_update().await;
    match ota_result {
        Err(e) => {
            warn!("Failed to update: {e:?}");
            wifi.stop().await?;
            info!("Wifi stopped");
            Err(anyhow::anyhow!("OTA update completed"))
        }
        Ok(_) => {
            info!("OTA update completed");
            wifi.stop().await?;
            info!("Wifi stopped");
            Ok(())
        }
    }
}

pub fn set_net_cmd(cmd: &str) -> bool {
    match global::CMD_NET.try_lock() {
        Ok(mut cmd_net) => {
            if cmd_net.as_str() != "" {
                warn!("CMD_NET in use as \"{cmd_net}\"");
                return false;
            }
            *cmd_net = cmd.to_string();
            true
        }
        Err(_) => {
            warn!("Failed to set CMD_NET");
            false
        }
    }
}

pub fn get_net_cmd() -> Result<String, String> {
    match global::CMD_NET.try_lock() {
        Ok(cmd_net) => Ok(cmd_net.clone()),
        Err(_) => Err("Failed to get CMD_NET".to_string()),
    }
}

fn clear_net_cmd() -> bool {
    match global::CMD_NET.try_lock() {
        Ok(mut cmd_net) => {
            *cmd_net = "".to_string();
            true
        }
        Err(_) => {
            warn!("Failed to clear CMD_NET");
            false
        }
    }
}

pub fn set_result_net(result: &str) -> bool {
    match global::RESULT_NET.lock() {
        Ok(mut result_net) => {
            *result_net = result.to_string();
            info!("RESULT_NET set to \"{result}\"");
            true
        }
        Err(_) => {
            warn!("Failed to set RESULT_NET to \"{result}\"");
            false
        }
    }
}

pub fn get_result_net() -> String {
    match global::RESULT_NET.try_lock() {
        Ok(result_net) => result_net.clone(),
        Err(_) => {
            warn!("Failed to get RESULT_NET");
            "".to_string()
        }
    }
}
