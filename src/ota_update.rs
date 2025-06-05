use embedded_svc::http::{client::Client, Method};
use esp_idf_svc::http::client::{Configuration as HttpConfiguration, EspHttpConnection};
use esp_idf_svc::ota::EspOta;
use log::info;

use std::time::Duration as StdDuration;

use crate::global;

pub async fn ota_update() -> anyhow::Result<()> {
    // https://hangulclock.homin.dev/v1/ping 이 200 을 반환할 때 까지 10초 간격으로 10회 시도해 보고 실패하면 오류를 반환하게 해 줘
    use std::thread::sleep;

    let ping_url = "https://hangulclock.homin.dev/v1/ping";
    let mut ping_success = false;
    for attempt in 1..=10 {
        let connection = EspHttpConnection::new(&HttpConfiguration {
            use_global_ca_store: true,
            crt_bundle_attach: Some(esp_idf_svc::sys::esp_crt_bundle_attach),
            timeout: Some(StdDuration::from_secs(10)),
            ..Default::default()
        })?;
        let mut client = Client::wrap(connection);
        let request = client.request(Method::Get, ping_url, &[])?;
        let response = request.submit()?;
        info!("Ping attempt {}: status {}", attempt, response.status());
        if response.status() == 200 {
            ping_success = true;
            break;
        }
        if attempt < 10 {
            sleep(StdDuration::from_secs(10));
        }
    }
    if !ping_success {
        anyhow::bail!("Failed to get 200 from /v1/ping after 10 attempts");
    }

    let connection = EspHttpConnection::new(&HttpConfiguration {
        use_global_ca_store: true,
        crt_bundle_attach: Some(esp_idf_svc::sys::esp_crt_bundle_attach),
        timeout: Some(StdDuration::from_secs(10)),
        ..Default::default()
    })?;
    let mut client = Client::wrap(connection);
    let url = format!(
        "https://hangulclock.homin.dev/v1/update?version={}",
        global::get_sw_version()
    );
    info!("Attempting to connect to {}", url);
    let request = client.request(Method::Get, url.as_ref(), &[] /*&headers*/)?;
    let mut response = request.submit()?;

    info!("Response code: {}", response.status());
    if response.status() == 200 {
        info!("Applying update...");
        let mut ota = EspOta::new().expect("obtain OTA instance");
        let mut update = ota.initiate_update().expect("initiate OTA");
        let mut buf = Box::new([0u8; 4096]);
        while let Ok(n) = response.read(&mut buf[..]) {
            if n == 0 {
                break;
            }
            info!("Writing OTA data: {}", n);
            update.write(&buf[..n]).expect("write OTA data");
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
