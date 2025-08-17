use embassy_time::{Duration, Timer};
use esp_idf_svc::hal::io::EspIOError;
use esp_idf_svc::http::server::{Configuration, EspHttpServer, Method};
use log::{info, warn};

pub async fn start_web_server() -> anyhow::Result<()> {
    // Set the HTTP server
    let mut server = EspHttpServer::new(&Configuration::default())?;
    // http://<sta ip>/ handler
    server.fn_handler(
        "/",
        Method::Get,
        |request| -> core::result::Result<(), EspIOError> {
            let html = index_html();
            let mut response = request.into_ok_response()?;
            response.write(html.as_bytes())?;
            Ok(())
        },
    )?;

    info!("Web server started");

    loop {
        // server.poll().await;
        Timer::after(Duration::from_millis(1000)).await;
    }
}

fn index_html() -> String {
    templated("Hello from mcu!")
}

fn templated(content: impl AsRef<str>) -> String {
    format!(
        r#"
<!DOCTYPE html>
<html>
    <head>
        <meta charset="utf-8">
        <title>esp-rs web server</title>
    </head>
    <body>
        {}
    </body>
</html>
"#,
        content.as_ref()
    )
}
