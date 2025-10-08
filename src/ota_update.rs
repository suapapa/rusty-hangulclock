use std::time::Duration as StdDuration;

use embedded_svc::http::client::Client;
use embedded_svc::http::Method;
use esp_idf_svc::http::client::{Configuration as HttpConfiguration, EspHttpConnection};
use esp_idf_svc::ota::EspOta;
use log::{debug, info, warn};

use crate::{global, net, timer};

pub async fn ota_update() -> anyhow::Result<()> {
    let ping_url = "https://hangulclock.homin.dev/v1/ping";
    let mut ping_success = false;
    for attempt in 1..=10 {
        info!("Ping attempt {attempt}: attempting to connect to {ping_url}");
        let connection = EspHttpConnection::new(&HttpConfiguration {
            use_global_ca_store: true,
            crt_bundle_attach: Some(esp_idf_svc::sys::esp_crt_bundle_attach),
            timeout: Some(StdDuration::from_secs(30)), // Increased from 10s to 30s
            ..Default::default()
        })?;
        let mut client = Client::wrap(connection);
        let request = client.request(Method::Get, ping_url, &[])?;
        info!("Ping attempt {attempt}: submitting request (may take up to 30s)...");
        let response = request.submit()?;
        info!("Ping attempt {}: status {}", attempt, response.status());
        if response.status() == 200 {
            ping_success = true;
            break;
        }
        if attempt < 10 {
            info!("Ping failed, retrying in 10 seconds...");
            timer::sleep_secs(10).await;
        }
    }
    if !ping_success {
        anyhow::bail!("Failed to get 200 from /v1/ping after 10 attempts");
    }

    let connection = EspHttpConnection::new(&HttpConfiguration {
        use_global_ca_store: true,
        crt_bundle_attach: Some(esp_idf_svc::sys::esp_crt_bundle_attach),
        timeout: Some(StdDuration::from_secs(30)), // Increased from 10s to 30s
        ..Default::default()
    })?;
    let mut client = Client::wrap(connection);
    let url = format!(
        "https://hangulclock.homin.dev/v1/update?version={}&rev={}",
        global::get_sw_version(),
        global::get_hw_revision(),
    );

    info!("Creating HTTP request for update (may take up to 30s to connect)...");
    let request = client.request(Method::Get, url.as_ref(), &[])?;
    info!("HTTP request created, now submitting (connection phase)...");
    let mut response = request.submit()?;
    timer::sleep_millis(10).await;

    info!("Response code: {}", response.status());
    if response.status() == 200 {
        info!("Applying update...");
        let mut ota = EspOta::new().expect("obtain OTA instance");
        let mut update = ota.initiate_update().expect("initiate OTA");
        let mut buf = Box::new([0u8; 4096]);
        let mut flashing_idx = 0;
        let spinner = ["-", "\\", "|", "/"];
        let mut spinner_index = 0;
        global::yield_to_other_tasks().await;

        let mut read_retry_cnt = 0;
        const READ_RETRY_CNT_MAX: u32 = 5;

        loop {
            match response.read(&mut buf[..]) {
                Ok(n) => {
                    if n == 0 {
                        break;
                    }
                    read_retry_cnt = 0;
                    debug!("Writing OTA data: {n}");
                    update.write(&buf[..n]).expect("write OTA data");
                    flashing_idx += 1;
                    if flashing_idx % 10 == 0 {
                        net::set_result_net(spinner[spinner_index]);
                        spinner_index = (spinner_index + 1) % spinner.len();
                        global::yield_to_other_tasks().await;
                    }
                }
                Err(e) => {
                    read_retry_cnt += 1;
                    warn!("Failed to read OTA data: {e} (retry {read_retry_cnt}/{READ_RETRY_CNT_MAX})");
                    if read_retry_cnt >= READ_RETRY_CNT_MAX {
                        anyhow::bail!("Failed to read OTA data: {e}");
                    }
                }
            }
        }
        update.complete().expect("complete OTA");
        info!("Update complete, rebooting...");
        esp_idf_svc::hal::reset::restart();
        // Ok(())
    } else {
        info!("No update available");
        Err(anyhow::anyhow!("No update available"))
    }
}
