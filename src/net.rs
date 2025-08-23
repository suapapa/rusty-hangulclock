use crate::global;
use crate::nvs;
use crate::ota_update;
use crate::report;
use crate::web_server;

use embassy_time::{Duration, Timer};
use embedded_svc::http::{client::Client, Method};
use embedded_svc::wifi::{
    AccessPointConfiguration, AuthMethod, ClientConfiguration, Configuration,
};
use esp_idf_svc::hal::sys::esp_wifi_set_max_tx_power;
use esp_idf_svc::http::client::{Configuration as HttpConfiguration, EspHttpConnection};
use esp_idf_svc::sntp;
use esp_idf_svc::wifi::{AsyncWifi, EspWifi};
use esp_idf_svc::wifi::{WpsConfig, WpsFactoryInfo, WpsStatus, WpsType};
use log::{info, warn};

use std::time::Duration as StdDuration;

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
            warn!("Failed to sync time: {:?}", e);
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
            let mut cmd_net = global::CMD_NET.lock().unwrap();

            match cmd_net.as_str() {
                "AP" => {
                    info!("Received AP command");
                    match connect_ap(wifi).await {
                        Ok(_) => {
                            info!("AP cmd completed");
                            *cmd_net = "".to_string();
                            let mut result = global::RESULT_NET.lock().unwrap();
                            *result = "OK".to_string();
                            ap_mode = true;
                        }
                        Err(e) => {
                            warn!("Failed to connect to wifi with ap: {:?}", e);
                            *cmd_net = "".to_string();
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
                            *cmd_net = "".to_string();
                            let mut result = global::RESULT_NET.lock().unwrap();
                            *result = "OK".to_string();
                        }
                        Err(e) => {
                            warn!("Failed to connect to wifi with wps: {:?}", e);
                            *cmd_net = "".to_string();
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
                            *cmd_net = "".to_string();
                            let mut result = global::RESULT_NET.lock().unwrap();
                            *result = "OK".to_string();
                        }
                        Err(e) => {
                            warn!("Failed to sync time: {:?}", e);
                            *cmd_net = "".to_string();
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
                            *cmd_net = "".to_string();
                            let mut result = global::RESULT_NET.lock().unwrap();
                            *result = "OK".to_string();
                        }
                        Err(e) => {
                            warn!("Failed to update: {:?}", e);
                            *cmd_net = "".to_string();
                            let mut result = global::RESULT_NET.lock().unwrap();
                            *result = "NG".to_string();
                        }
                    }
                }

                _ => {
                    // warn!("Unknown command: \"{}\"", cmd_net);
                }
            }
        }

        // debug_led.set_low().unwrap();
    }
}

const WPS_CONFIG: WpsConfig = WpsConfig {
    wps_type: WpsType::Pbc,
    factory_info: WpsFactoryInfo {
        manufacturer: "homin.dev",
        model_number: "hangulclock202505",
        model_name: "Rusty HangulClock",
        device_name: "Rusty HangulClock",
    },
};

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

    info!("Starting WPS...");
    match wifi.start_wps(&WPS_CONFIG).await? {
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
    Timer::after(Duration::from_secs(2)).await;
    info!("Network initialization delay completed");

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
            warn!("Failed to load wifi cred: {:?}", e);
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
    Timer::after(Duration::from_secs(2)).await;
    info!("Network initialization delay completed");

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
            warn!("Failed to load wifi cred: {:?}", e);
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
    Timer::after(Duration::from_secs(2)).await;
    info!("Network initialization delay completed");

    send_report_without_wifi().await?;

    wifi.stop().await?;
    info!("Wifi stopped");

    Ok(())
}

async fn send_report_without_wifi() -> anyhow::Result<()> {
    let connection = EspHttpConnection::new(&HttpConfiguration {
        use_global_ca_store: true,
        crt_bundle_attach: Some(esp_idf_svc::sys::esp_crt_bundle_attach),
        timeout: Some(StdDuration::from_secs(10)),
        ..Default::default()
    })?;
    let mut client = Client::wrap(connection);

    let auth_header = format!("Bearer {}", get_api_token());
    let headers = [
        ("Content-Type", "application/json"),
        ("Authorization", auth_header.as_str()),
    ];

    let url = "https://hangulclock.homin.dev/v1/live-status";
    info!("Attempting to connect to {}", url);
    let mut request = client.request(Method::Post, url.as_ref(), &headers)?;

    info!("Sending report data");
    let report_json = report::status_report().await?;
    request.write(report_json.as_bytes())?;
    request.flush()?;

    info!("Waiting for response");
    let response = request.submit()?;
    let status = response.status();

    info!("Response code: {}", status);

    Ok(())
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
            warn!("Failed to load wifi cred: {:?}", e);
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
    Timer::after(Duration::from_secs(2)).await;
    info!("Network initialization delay completed");

    let ota_result = ota_update::ota_update().await;
    match ota_result {
        Err(e) => {
            warn!("Failed to update: {:?}", e);
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
