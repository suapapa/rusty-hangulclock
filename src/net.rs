use std::time::Duration as StdDuration;

use embassy_time::{Duration, Timer};
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

use crate::{global, nvs, ota_update, report, web_server};

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
    info!("initial time sync...");
    match sync_time_with_wifi(wifi).await {
        Ok(_) => (),
        Err(e) => {
            warn!("Failed to sync time: {e:?}");
        }
    }

    let mut ap_mode = false;
    // Watchdog 카운터 추가
    let mut watchdog_counter = 0;
    const WATCHDOG_INTERVAL: u32 = 10000; // 100ms * 10000 = 1000초마다 체크

    loop {
        Timer::after(Duration::from_millis(100)).await;

        // Watchdog 체크
        watchdog_counter += 1;
        if watchdog_counter >= WATCHDOG_INTERVAL {
            info!("Net loop watchdog reset");
            watchdog_counter = 0;
        }

        if ap_mode {
            continue;
        }

        {
            let cmd_net = {
                let mut cmd_net = global::CMD_NET.lock().unwrap();
                let ret = cmd_net.clone();
                *cmd_net = "".to_string();
                ret
            };

            match cmd_net.as_str() {
                "AP" => {
                    info!("Received AP command");
                    match connect_ap(wifi).await {
                        Ok(_) => {
                            info!("AP cmd completed");
                            let mut result = global::RESULT_NET.lock().unwrap();
                            *result = "OK".to_string();
                            ap_mode = true;
                        }
                        Err(e) => {
                            warn!("Failed to connect to wifi with ap: {e:?}");
                            let mut result = global::RESULT_NET.lock().unwrap();
                            *result = "NG".to_string();
                        }
                    }
                }
                "WPS" => {
                    info!("Received WPS command");
                    match connect_wps(wifi).await {
                        Ok(_) => {
                            info!("WPS cmd completed");
                            let mut result = global::RESULT_NET.lock().unwrap();
                            *result = "OK".to_string();
                        }
                        Err(e) => {
                            warn!("Failed to connect to wifi with wps: {e:?}");
                            let mut result = global::RESULT_NET.lock().unwrap();
                            *result = "NG".to_string();
                        }
                    }
                }
                "NTP" => {
                    info!("Received NTP command");
                    send_report(wifi).await?;

                    match sync_time_with_wifi(wifi).await {
                        Ok(_) => {
                            info!("NTP cmd completed");
                            let mut result = global::RESULT_NET.lock().unwrap();
                            *result = "OK".to_string();
                        }
                        Err(e) => {
                            warn!("Failed to sync time: {e:?}");
                            let mut result = global::RESULT_NET.lock().unwrap();
                            *result = "NG".to_string();
                        }
                    }
                }
                "OTA" => {
                    info!("Received OTA command");
                    match ota_update_with_wifi(wifi).await {
                        Ok(_) => {
                            info!("OTA cmd completed");
                            let mut result = global::RESULT_NET.lock().unwrap();
                            *result = "OK".to_string();
                        }
                        Err(e) => {
                            warn!("Failed to update: {e:?}");
                            let mut result = global::RESULT_NET.lock().unwrap();
                            *result = "NG".to_string();
                        }
                    }
                }
                "" => {
                    debug!("Received empty command");
                }
                _ => {
                    warn!("Unknown command: \"{cmd_net}\"");
                }
            }
        }

        // debug_led.set_low().unwrap();
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

    match wifi.start_wps(&wps_config).await? {
        WpsStatus::SuccessConnected => (),
        WpsStatus::SuccessMultipleAccessPoints(credentials) => {
            log::info!("received multiple credentials, connecting to first one:");
            for i in &credentials {
                log::info!(" - ssid: {}", i.ssid);
            }
            let wifi_configuration: Configuration = Configuration::Client(ClientConfiguration {
                ssid: credentials[0].ssid.clone(),
                bssid: None,
                auth_method: AuthMethod::WPA2Personal,
                password: credentials[1].passphrase.clone(),
                channel: None,
                ..Default::default()
            });
            wifi.set_configuration(&wifi_configuration)?;
        }
        WpsStatus::Failure => anyhow::bail!("WPS failure"),
        WpsStatus::Timeout => anyhow::bail!("WPS timeout"),
        WpsStatus::Pin(_) => anyhow::bail!("WPS pin"),
        WpsStatus::PbcOverlap => anyhow::bail!("WPS PBC overlap"),
    }

    match wifi.get_configuration()? {
        Configuration::Client(config) => {
            info!("Successfully connected to {} using WPS", config.ssid);
            nvs::set_wifi_cred(&config.ssid.clone(), &config.password.clone())?;
        }
        _ => anyhow::bail!("Not in station mode"),
    };

    wifi.connect().await?;
    info!("Wifi connected");

    wifi.wait_netif_up().await?;
    info!("Wifi netif up");

    // Add delay to ensure network is fully initialized
    Timer::after(Duration::from_secs(3)).await; // Increased from 2s to 3s
    info!("Network initialization delay completed");

    // Additional network stability check
    Timer::after(Duration::from_millis(500)).await;
    info!("Additional network stability delay completed");

    sync_time().await;
    info!("Time synced");

    send_report_without_wifi().await?;

    wifi.stop().await?;
    info!("Wifi stopped");

    Ok(())
}

pub async fn sync_time_with_wifi(wifi: &mut AsyncWifi<EspWifi<'static>>) -> anyhow::Result<bool> {
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

    // Add delay to ensure network is fully initialized
    Timer::after(Duration::from_secs(3)).await; // Increased from 2s to 3s
    info!("Network initialization delay completed");

    // Additional network stability check
    Timer::after(Duration::from_millis(500)).await;
    info!("Additional network stability delay completed");

    let sync_result = sync_time().await;
    if !sync_result {
        warn!("Failed to sync time");
    }

    wifi.stop().await?;
    info!("Wifi stopped");

    Ok(sync_result)
}

async fn sync_time() -> bool {
    let sntp_conf = sntp::SntpConf {
        servers: ["time.google.com"], // "pool.ntp.org"
        operating_mode: sntp::OperatingMode::Poll,
        sync_mode: sntp::SyncMode::Immediate,
    };

    let sntp = sntp::EspSntp::new(&sntp_conf).expect("Failed to create SNTP");
    let mut ret = false;
    let mut retry = 10;
    loop {
        if retry == 0 {
            break;
        }
        if sntp.get_sync_status() == sntp::SyncStatus::Completed {
            info!("SNTP synced");
            ret = true;
            break;
        }
        info!("Waiting for SNTP sync...");
        Timer::after(Duration::from_secs(3)).await;
        retry -= 1;
    }

    {
        info!("Setting time_synced");
        let mut time_synced = global::TIME_SYNCED.lock().unwrap();
        *time_synced = ret;
    }

    ret
}

pub async fn send_report(wifi: &mut AsyncWifi<EspWifi<'static>>) -> anyhow::Result<()> {
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

    // Add delay to ensure network is fully initialized
    Timer::after(Duration::from_secs(3)).await; // Increased from 2s to 3s
    info!("Network initialization delay completed");

    // Additional network stability check
    Timer::after(Duration::from_millis(500)).await;
    info!("Additional network stability delay completed");

    send_report_without_wifi().await?;

    wifi.stop().await?;
    info!("Wifi stopped");

    Ok(())
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
                    Timer::after(Duration::from_millis(RETRY_DELAY_MS)).await;
                } else {
                    warn!("All {MAX_RETRIES} attempts failed, giving up");
                    return Err(e);
                }
            }
        }
    }

    unreachable!()
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

    // Add delay to ensure network is fully initialized
    Timer::after(Duration::from_secs(3)).await; // Increased from 2s to 3s
    info!("Network initialization delay completed");

    // Additional network stability check
    Timer::after(Duration::from_millis(500)).await;
    info!("Additional network stability delay completed");

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
