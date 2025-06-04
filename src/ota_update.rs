use embedded_svc::http::{client::Client, Method};
use esp_idf_svc::http::client::{Configuration as HttpConfiguration, EspHttpConnection};
use log::info;

use std::time::Duration as StdDuration;

pub async fn ota_update() -> anyhow::Result<()> {
    let connection = EspHttpConnection::new(&HttpConfiguration {
        use_global_ca_store: true,
        crt_bundle_attach: Some(esp_idf_svc::sys::esp_crt_bundle_attach),
        timeout: Some(StdDuration::from_secs(10)),
        ..Default::default()
    })?;
    let mut client = Client::wrap(connection);
    let url = format!(
        "https://hangulclock.homin.dev/v1/update?version={}",
        get_sw_version()
    );
    info!("Attempting to connect to {}", url);
    let request = client.request(Method::Get, url.as_ref(), &[] /*&headers*/)?;
    let response = request.submit()?;

    info!("Response code: {}", response.status());
    if response.status() == 302 {
        let connection = EspHttpConnection::new(&HttpConfiguration {
            use_global_ca_store: true,
            crt_bundle_attach: Some(esp_idf_svc::sys::esp_crt_bundle_attach),
            timeout: Some(StdDuration::from_secs(10)),
            ..Default::default()
        })?;
        let mut client = Client::wrap(connection);
        let location = response.header("Location").unwrap();
        info!("Redirecting to {}", location);
        let request = client.request(Method::Get, location, &[] /*&headers*/)?;
        let response = request.submit()?;
        let status = response.status();
        info!("Redirected response code: {}", status);
    } else {
        info!("No update available");
    }

    Ok(())
}

pub fn get_sw_version() -> i32 {
    match option_env!("RUSTY_HANGULCLOCK_SW_VERSION") {
        Some(s) => match s.parse::<i32>() {
            Ok(v) => v,
            Err(_) => 0,
        },
        None => 0,
    }
}
