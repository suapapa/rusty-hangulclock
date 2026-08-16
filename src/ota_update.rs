use std::time::Duration as StdDuration;

use embedded_svc::http::client::Client;
use embedded_svc::http::Method;
use esp_idf_svc::http::client::{Configuration as HttpConfiguration, EspHttpConnection};
use esp_idf_svc::ota::EspOta;
use log::info;

use crate::{global, net, timer};

pub async fn ota_update() -> anyhow::Result<()> {
    const PING_URL: &str = "https://hangulclock.homin.dev/v1/ping";
    let mut ping_success = false;

    let http_config = HttpConfiguration {
        use_global_ca_store: true,
        crt_bundle_attach: Some(esp_idf_svc::sys::esp_crt_bundle_attach),
        timeout: Some(StdDuration::from_secs(30)),
        ..Default::default()
    };

    for attempt in 1..=5 {
        global::heartbeat(global::TaskId::Net);
        info!("Ping attempt {}: connecting to {}", attempt, PING_URL);
        let connection = EspHttpConnection::new(&http_config)?;
        let mut client = Client::wrap(connection);

        if let Ok(request) = client.request(Method::Get, PING_URL, &[]) {
            if let Ok(response) = request.submit() {
                if response.status() == 200 {
                    ping_success = true;
                    break;
                }
            }
        }
        timer::sleep_secs(5).await;
    }

    if !ping_success {
        anyhow::bail!("Failed to reach update server after 5 attempts");
    }

    let update_url = format!(
        "https://hangulclock.homin.dev/v1/update?version={}&rev={}",
        global::get_sw_version(),
        global::get_hw_revision(),
    );

    let ota_config = HttpConfiguration {
        timeout: Some(StdDuration::from_secs(300)), // 5 mins
        ..http_config
    };

    let connection = EspHttpConnection::new(&ota_config)?;
    let mut client = Client::wrap(connection);
    let request = client.request(Method::Get, &update_url, &[])?;
    let mut response = request.submit()?;

    if response.status() != 200 {
        info!("No update available or server error: {}", response.status());
        return Ok(());
    }

    info!("Applying update...");
    let mut ota = EspOta::new()?;
    let mut update = ota.initiate_update()?;

    let mut buf = [0u8; 4096];
    let mut total_downloaded = 0usize;
    let total_size = response
        .header("Content-Length")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0);

    let mut watchdog = global::WatchdogManager::new(global::TaskId::Net, 100, 10);

    loop {
        if watchdog.update() {
            global::yield_to_other_tasks().await;
        }

        let n = response.read(&mut buf)?;
        if n == 0 {
            break;
        }

        update.write(&buf[..n])?;
        total_downloaded += n;

        if total_size > 0 && total_downloaded % 16384 == 0 {
            let progress = (total_downloaded * 100) / total_size;
            net::set_result_net(&format!("{}%", progress));
            info!("Progress: {}%", progress);
        }
    }

    update.complete()?;
    net::set_result_net("OK");
    info!("Update complete, rebooting...");
    timer::sleep_secs(2).await;
    esp_idf_svc::hal::reset::restart();
    // Ok(())
}
