use esp_idf_svc::hal::io::EspIOError;
use esp_idf_svc::http::server::{Configuration, EspHttpServer, Method};
use log::{info, warn};

use crate::{global, timer};

pub async fn start_web_server() -> anyhow::Result<()> {
    // Set the HTTP server
    let mut server = EspHttpServer::new(&Configuration::default())?;

    // http://<sta ip>/ handler - GET for form display
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

    // http://<sta ip>/ handler - POST for WiFi credentials
    server.fn_handler(
        "/",
        Method::Post,
        |mut request| -> core::result::Result<(), EspIOError> {
            let mut buffer = [0u8; 512];
            let mut total_read = 0;

            // Read the POST data
            loop {
                match request.read(&mut buffer[total_read..]) {
                    Ok(0) => break, // EOF
                    Ok(n) => {
                        total_read += n;
                        if total_read >= buffer.len() {
                            warn!("POST data buffer full, truncating");
                            break; // Buffer full
                        }
                    }
                    Err(e) => {
                        warn!("Error reading POST data: {e}");
                        break;
                    }
                }
            }

            let post_data = String::from_utf8_lossy(&buffer[..total_read]);
            info!("Received POST data length: {total_read} bytes");

            // Only log first 100 chars to avoid excessive logging
            if post_data.len() > 100 {
                info!("POST data preview: {}...", &post_data[..100]);
            } else {
                info!("POST data: {post_data}");
            }

            // Parse form data (simple key=value&key=value format)
            let mut ssid = String::new();
            let mut pass = String::new();

            for pair in post_data.split('&') {
                if let Some((key, value)) = pair.split_once('=') {
                    match key.trim() {
                        "ssid" => {
                            // First replace + with spaces, then URL decode
                            let decoded_value = value.trim().replace('+', " ");
                            ssid = urlencoding::decode(&decoded_value).unwrap_or_default().to_string();
                            info!("Parsed SSID: {} (length: {})", ssid, ssid.len());
                        }
                        "pass" => {
                            // First replace + with spaces, then URL decode
                            let decoded_value = value.trim().replace('+', " ");
                            pass = urlencoding::decode(&decoded_value).unwrap_or_default().to_string();
                            info!("Parsed password length: {}", pass.len());
                        }
                        _ => {}
                    }
                }
            }

            // Call NVS function to store WiFi credentials
            if !ssid.is_empty() && !pass.is_empty() {
                info!("Attempting to store WiFi credentials - SSID: {} ({} chars), Password: {} chars",
                      ssid, ssid.len(), pass.len());

                match crate::nvs::set_wifi_cred(&ssid, &pass) {
                    Ok(_) => {
                        info!("WiFi credentials stored successfully");
                        let response_html = success_html(&ssid);
                        let mut response = request.into_ok_response()?;
                        response.write(response_html.as_bytes())?;
                    }
                    Err(e) => {
                        warn!("Failed to store WiFi credentials: {e}");
                        let response_html =
                            error_html(&format!("Failed to store credentials: {e}"));
                        let mut response = request.into_ok_response()?;
                        response.write(response_html.as_bytes())?;
                    }
                }
            } else {
                warn!("Empty SSID or password received - SSID: '{ssid}', Password: '{pass}'");
                let response_html = error_html("SSID and password are required");
                let mut response = request.into_ok_response()?;
                response.write(response_html.as_bytes())?;
            }

            Ok(())
        },
    )?;

    // http://<sta ip>/current-cred handler - GET current WiFi credentials
    server.fn_handler(
        "/current-cred",
        Method::Get,
        |request| -> core::result::Result<(), EspIOError> {
            let cred_html = match crate::nvs::get_wifi_cred() {
                Ok((ssid, _)) => {
                    format!("<p><strong>SSID:</strong> {ssid}</p>")
                }
                Err(_) => "<p style='color: #999;'>저장된 WiFi 정보가 없습니다.</p>".to_string(),
            };

            let mut response = request.into_ok_response()?;
            response.write(cred_html.as_bytes())?;
            Ok(())
        },
    )?;

    info!("Web server started");

    // Watchdog manager (1초 * 60 = 60초마다 체크, 10회마다 yield)
    let mut watchdog = global::WatchdogManager::new(global::TaskId::Net, 60, 10);

    loop {
        // Watchdog 체크 및 yield
        if watchdog.update() {
            global::yield_to_other_tasks().await;
        }

        // server.poll().await;
        timer::sleep_secs(1).await;
    }
}

fn index_html() -> String {
    templated(
        r#"
        <h2>WiFi 설정</h2>
        <form id="wifi-form">
            <div style="margin-bottom: 15px;">
                <label for="ssid">WiFi SSID:</label><br>
                <input type="text" id="ssid" name="ssid" required style="width: 250px; padding: 8px; margin-top: 5px;">
            </div>
            <div style="margin-bottom: 15px;">
                <label for="pass">WiFi Password:</label><br>
                <input type="password" id="pass" name="pass" required style="width: 250px; padding: 8px; margin-top: 5px;">
            </div>
            <button type="submit" style="padding: 10px 20px; background-color: #4CAF50; color: white; border: none; cursor: pointer;">
                저장
            </button>
        </form>
        <div style="margin-top: 20px; font-size: 14px; color: #666;">
            <p>현재 저장된 WiFi 정보:</p>
            <div id="current-cred"></div>
        </div>
        <script>
            // 현재 저장된 WiFi 정보 표시 (선택사항)
            fetch('/current-cred')
                .then(response => response.text())
                .then(data => {
                    document.getElementById('current-cred').innerHTML = data;
                })
                .catch(error => {
                    console.log('Error fetching current credentials:', error);
                });
            
            // 폼 제출을 JavaScript로 처리하여 더 안전하게 전송
            document.querySelector('form').addEventListener('submit', function(e) {
                e.preventDefault();
                
                const formData = new FormData(this);
                const ssid = formData.get('ssid');
                const pass = formData.get('pass');
                
                if (!ssid || !pass) {
                    alert('SSID와 패스워드를 모두 입력해주세요.');
                    return;
                }
                
                // POST 요청으로 데이터 전송
                fetch('/', {
                    method: 'POST',
                    headers: {
                        'Content-Type': 'application/x-www-form-urlencoded',
                    },
                    body: new URLSearchParams({
                        'ssid': ssid,
                        'pass': pass
                    })
                })
                .then(response => response.text())
                .then(html => {
                    document.body.innerHTML = html;
                })
                .catch(error => {
                    console.error('Error:', error);
                    alert('WiFi 설정 저장 중 오류가 발생했습니다.');
                });
            });
        </script>
        "#,
    )
}

fn success_html(ssid: &str) -> String {
    templated(format!(
        r#"
            <h2 style="color: #4CAF50;">✅ WiFi 설정이 저장되었습니다!</h2>
            <p><strong>SSID:</strong> {ssid}</p>
            <p>이제 디바이스가 재부팅되면 해당 WiFi에 연결을 시도합니다.</p>
            <button onclick="window.location.href='/'" style="display: inline-block; padding: 10px 20px; background-color: #2196F3; color: white; text-decoration: none; margin-top: 15px; border: none; cursor: pointer; border-radius: 4px;">
                돌아가기
            </button>
            <script>
                // 5초 후 자동으로 메인 페이지로 이동
                setTimeout(() => {{
                    window.location.href = '/';
                }}, 5000);
            </script>
            "#
    ))
}

fn error_html(message: &str) -> String {
    templated(format!(
        r#"
            <h2 style="color: #f44336;">❌ 오류가 발생했습니다</h2>
            <p>{message}</p>
            <a href="/" style="display: inline-block; padding: 10px 20px; background-color: #2196F3; color: white; text-decoration: none; margin-top: 15px;">
                다시 시도
            </a>
            "#
    ))
}

fn templated(content: impl AsRef<str>) -> String {
    format!(
        r#"
<!DOCTYPE html>
<html>
    <head>
        <meta charset="utf-8">
        <title>Rusty Hangul Clock - WiFi 설정</title>
        <meta name="viewport" content="width=device-width, initial-scale=1">
        <style>
            body {{
                font-family: Arial, sans-serif;
                max-width: 600px;
                margin: 0 auto;
                padding: 20px;
                background-color: #f5f5f5;
            }}
            .container {{
                background-color: white;
                padding: 30px;
                border-radius: 8px;
                box-shadow: 0 2px 10px rgba(0,0,0,0.1);
            }}
            input[type="text"], input[type="password"] {{
                border: 1px solid #ddd;
                border-radius: 4px;
                font-size: 16px;
            }}
            button:hover {{
                background-color: #45a049 !important;
            }}
            a:hover {{
                background-color: #1976D2 !important;
            }}
        </style>
    </head>
    <body>
        <div class="container">
            {}
        </div>
    </body>
</html>
"#,
        content.as_ref()
    )
}
