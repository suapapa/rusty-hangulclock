mod global;
mod menu;
mod net;
mod nvs;
mod ota_update;
mod panel;
mod report;
mod rotary;
mod timer;
mod web_server;

// use smart_leds::{gamma, hsv::hsv2rgb, hsv::Hsv, SmartLedsWrite, RGB8};
use std::time;

#[cfg(feature = "dotstar")]
use apa102_spi::MODE as SPI_MODE;
use chrono::prelude::*;
use esp_idf_svc::eventloop::EspSystemEventLoop;
// use embedded_hal::spi::MODE_3;
use esp_idf_svc::hal::gpio::*;
use esp_idf_svc::hal::i2c::*;
use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::hal::prelude::*;
use esp_idf_svc::hal::reset::restart;
use esp_idf_svc::hal::spi::config::{Config as SpiConfig, DriverConfig as SpiDriverConfig};
use esp_idf_svc::hal::spi::{SpiBusDriver, SpiDriver};
use esp_idf_svc::nvs::EspDefaultNvsPartition;
// use esp_idf_svc::sys::{esp_task_wdt_add, esp_task_wdt_delete, xTaskGetCurrentTaskHandle};
use esp_idf_svc::timer::EspTaskTimerService;
use esp_idf_svc::wifi::{AsyncWifi, EspWifi};
use log::{debug, info, warn};
use sh1106::prelude::GraphicsMode as Sh1106GM;
use sh1106::Builder as Sh1106Builder;
#[cfg(feature = "neopixel")]
use ws2812_spi::MODE as SPI_MODE;

fn main() -> anyhow::Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    // Set panic hook to restart device on panic
    // Note: Skip restart if in AP_MODE or OTA_MODE to avoid interrupting network
    // operations
    std::panic::set_hook(Box::new(|panic_info| {
        // Check if this is a task watchdog timeout
        let panic_msg = format!("{panic_info:#?}");
        let is_wdt_timeout = panic_msg.contains("task_wdt")
            || panic_msg.contains("Task watchdog")
            || panic_msg.contains("watchdog")
            || panic_info
                .payload()
                .downcast_ref::<&str>()
                .map(|s| s.contains("task_wdt") || s.contains("watchdog"))
                .unwrap_or(false);

        if is_wdt_timeout {
            log::error!("Task Watchdog Timer timeout detected!");
            log::error!("A task failed to reset the watchdog within the timeout period");
            log::error!("This usually indicates a task is stuck or not properly yielding");
            log::error!("Check CONFIG_ESP_TASK_WDT_TIMEOUT_S for the configured timeout");
            log::error!("Panic details: {panic_info:#?}");
        } else {
            log::error!("Panic occurred: {panic_info:#?}");
        }

        // Check if we're in AP_MODE or OTA_MODE
        let in_special_mode = {
            let ap_mode = global::AP_MODE.try_lock().map(|v| *v).unwrap_or(false);
            let ota_mode = global::OTA_MODE.try_lock().map(|v| *v).unwrap_or(false);
            ap_mode || ota_mode
        };

        if in_special_mode {
            log::warn!("Panic in AP_MODE or OTA_MODE - skipping restart to allow network operations to complete");
            log::warn!("Device will continue running (may be unstable)");
        } else {
            log::error!("Restarting device...");
            restart();
        }
    }));

    info!("Hello, RustyHangulClock!");

    let p = Peripherals::take()?;

    let p_oled_sda = p.pins.gpio8;
    let p_oled_scl = p.pins.gpio9;
    // let p_oled_res = p.pins.gpio10;
    let p_sled_sclk = p.pins.gpio4;
    let p_sled_mosi = p.pins.gpio6;
    let p_sled_spi = p.spi2;
    let p_menu_r1 = p.pins.gpio2;
    let p_menu_r2 = p.pins.gpio1;
    let p_menu_sel = p.pins.gpio3;

    let mut menu_sel = PinDriver::input(p_menu_sel)?;
    menu_sel.set_pull(Pull::Up)?;

    let mut menu_r1 = PinDriver::input(p_menu_r1)?;
    menu_r1.set_pull(Pull::Up)?;
    let mut menu_r2 = PinDriver::input(p_menu_r2)?;
    menu_r2.set_pull(Pull::Up)?;

    // reset oled display
    // let mut disp_res = PinDriver::output(p_oled_res)?;
    // disp_res.set_low().unwrap();
    // timer::sleep_millis(100);
    // disp_res.set_high().unwrap();

    let i2c_config = I2cConfig::new().baudrate(50.kHz().into());
    let i2c = I2cDriver::new(
        p.i2c0,
        p_oled_sda, // SDA
        p_oled_scl, // SCL
        &i2c_config,
    )?;
    let mut disp: Sh1106GM<_> = Sh1106Builder::new().connect_i2c(i2c).into();
    disp.init().unwrap();

    let hw_rev = global::get_hw_revision();
    info!("hw_rev: {hw_rev}");
    match hw_rev {
        4 => disp
            .set_rotation(sh1106::prelude::DisplayRotation::Rotate270)
            .unwrap(),

        // 3
        _ => disp
            .set_rotation(sh1106::prelude::DisplayRotation::Rotate90)
            .unwrap(),
    }
    disp.flush().unwrap();
    menu::draw_text(
        &mut disp,
        &format!(
            "Rusty\nHangul\nClock\nrev.{}\n\nno.{}\nver.{}\n\ninit...",
            global::get_hw_revision(),
            nvs::get_device_no()?,
            global::get_sw_version()
        ),
    )?;

    // let spi_driver_config = SpiDriverConfig::new().dma(Dma::Auto(1024));
    let spi_driver_config = SpiDriverConfig::new();

    let mut spi_driver = SpiDriver::new(
        p_sled_spi,
        p_sled_sclk,
        p_sled_mosi,
        AnyIOPin::none(),
        &spi_driver_config,
    )?;
    let spi_config = SpiConfig::new()
        .baudrate(3200.kHz().into()) // 2M ~ 3.8M
        .data_mode(SPI_MODE);
    let spi_bus = SpiBusDriver::new(&mut spi_driver, &spi_config)?;
    let mut sleds = panel::Sleds::new(spi_bus);
    sleds.welcome();
    let sys_loop = EspSystemEventLoop::take()?;
    let timer_service = EspTaskTimerService::new()?;
    let nvs = EspDefaultNvsPartition::take()?;
    let mut wifi = AsyncWifi::wrap(
        EspWifi::new(p.modem, sys_loop.clone(), Some(nvs))?,
        sys_loop,
        timer_service,
    )?;

    futures::executor::block_on(async {
        inc_boot_count().await?;
        Ok::<(), anyhow::Error>(())
    })?;

    info!("Starting tasks...");
    // Register main task with Task Watchdog Timer
    let _wdt_registered = global::register_task_with_wdt("main");
    if !_wdt_registered {
        warn!("Failed to register with TWDT - will run without watchdog protection");
    }

    futures::executor::block_on(async {
        let net_task = net::net_loop(&mut wifi);
        let show_time_task = show_time_loop(&mut sleds);
        let menu_task = menu::menu_loop(&mut disp, menu_sel);
        let time_sync_task = time_sync_loop();
        let rotary_encoder_task = rotary::rotary_encoder_loop(menu_r2, menu_r1);

        // Note: All *_loop() functions run as async tasks within a single FreeRTOS task
        // (main). They are NOT separate FreeRTOS tasks, so
        // reset_task_watchdog() resets the same main task's watchdog. Task
        // Watchdog Timer is currently disabled (CONFIG_ESP_TASK_WDT=n),
        // so WatchdogManager is used for cooperative multitasking and periodic yields.

        match futures::try_join!(
            menu_task,
            net_task,
            time_sync_task,
            show_time_task,
            rotary_encoder_task,
        ) {
            Ok(_) => info!("All tasks completed"),
            Err(e) => warn!("Error in task: {e:?}"),
        }
    });

    info!("Restarting...");
    esp_idf_svc::hal::reset::restart();
    // Ok(())
}

async fn time_sync_loop() -> anyhow::Result<()> {
    info!("Starting time_sync_loop()...");

    let sync_check_interval_secs = 60; // 1 hour
    let mut sync_check_cnt = 0;

    // Watchdog manager (60초마다 체크, 10회마다 yield)
    let mut watchdog = global::WatchdogManager::new(60, 10);

    loop {
        timer::sleep_secs(sync_check_interval_secs).await;

        // Watchdog 체크 및 yield
        if watchdog.update() {
            global::yield_to_other_tasks().await;
        }

        // 네트워크 명령 체크
        if net::check_net_cmd_or_skip().await.is_err() {
            timer::sleep_millis(50).await;
            continue;
        }

        // Every 3 days
        sync_check_cnt += 1;
        if sync_check_cnt >= 60 * 24 * 3 {
            sync_check_cnt = 0;
        }

        if sync_check_cnt != 0 {
            continue;
        }

        info!("Syncing time...");

        // esp_idf_svc::hal::reset::restart();

        if !net::set_net_cmd("NTP") {
            warn!("Failed to send NTP cmd");
            timer::sleep_secs(1).await;
            continue;
        }

        // NTP 동기화 루프에 타임아웃 추가
        let mut timeout_count = 0;
        const MAX_TRIES: u8 = 6; // 30초 타임아웃

        info!("Waiting for NTP cmd result...");
        loop {
            let result = net::get_result_net();
            let result_str = result.as_str();
            if result_str == "OK" || result_str == "NG" {
                info!("NTP cmd completed: {}", result.as_str());
                timer::sleep_secs(1).await;
                break;
            }
            info!("NTP cmd result: {result_str}");
            timer::sleep_secs(5).await;
            timeout_count += 1;
            info!("Timeout count: {timeout_count}");
            if timeout_count >= MAX_TRIES {
                warn!("NTP sync timeout, breaking loop");
                break;
            }
        }
        info!("NTP cmd completed");
        net::set_result_net("");
    }
}

async fn inc_boot_count() -> anyhow::Result<()> {
    let boot_count = nvs::get_boot_count()?;
    nvs::set_boot_count(boot_count + 1)?;
    Ok(())
}

async fn show_time_loop<SPI>(sleds: &mut panel::Sleds<SPI>) -> anyhow::Result<()>
where
    SPI: embedded_hal::spi::SpiBus,
{
    info!("Starting show_time_loop()...");

    let mut skip_display: bool; // = false;
    let mut last_h: u8 = 0;
    let mut last_m: u8 = 0;

    // Watchdog manager (60초마다 체크, 10회마다 yield)
    let mut watchdog = global::WatchdogManager::new(60, 10);

    let utc_offset: i32 = nvs::get_utc_offset()?;

    loop {
        // Watchdog 체크 및 yield
        if watchdog.update() {
            global::yield_to_other_tasks().await;
        }

        // 네트워크 명령 체크
        if net::check_net_cmd_or_skip().await.is_err() {
            timer::sleep_secs(1).await;
            continue;
        }

        match global::IN_MENU.try_lock() {
            Ok(in_menu) => {
                skip_display = *in_menu;
            }
            Err(_) => {
                debug!("IN_MENU in use");
                timer::sleep_secs(1).await;
                continue;
            }
        }

        match global::TIME_SYNCED.try_lock() {
            Ok(time_synced) => {
                if !*time_synced {
                    skip_display = true;
                }
            }
            Err(_) => {
                debug!("TIME_SYNCED in use");
                timer::sleep_secs(1).await;
                continue;
            }
        }

        if skip_display {
            sleds.turn_on_all();
            last_h = 0;
            last_m = 0;
            timer::sleep_secs(1).await;
            continue;
        }

        let now = time::SystemTime::now();
        let timestamp = now.duration_since(time::UNIX_EPOCH).unwrap().as_millis();

        let datetime = Utc.timestamp_millis_opt(timestamp as i64).unwrap();
        let local_datetime =
            datetime.with_timezone(&FixedOffset::east_opt(utc_offset * 3600).unwrap());

        let h: u8 = local_datetime.hour() as u8;
        let m: u8 = local_datetime.minute() as u8;
        if last_h != h || last_m != m {
            last_h = h;
            last_m = m;
            {
                // h, m 값을 전역 변수에 저장
                if let Ok(mut global_h) = global::CUR_H.try_lock() {
                    *global_h = h;
                } else {
                    debug!("Failed to update global H value");
                }
                if let Ok(mut global_m) = global::CUR_M.try_lock() {
                    *global_m = m;
                } else {
                    debug!("Failed to update global M value");
                }
            }
            debug!("Time updated, h: {h}, m: {m}");
            sleds.show_time(h, m);
        }
        timer::sleep_secs(1).await;
    }

    // Note: TWDT unregistration is handled automatically when task ends
    // Ok(())
}
