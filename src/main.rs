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

#[cfg(feature = "dotstar")]
use apa102_spi::MODE as SPI_MODE;
use chrono::prelude::*;
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::gpio::*;
use esp_idf_svc::hal::i2c::*;
use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::hal::reset::restart;
use esp_idf_svc::hal::spi::config::{Config as SpiConfig, DriverConfig as SpiDriverConfig};
use esp_idf_svc::hal::spi::{SpiBusDriver, SpiDriver};
use esp_idf_svc::hal::units::*;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
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

    setup_panic_hook();
    info!("Hello, RustyHangulClock!");

    let p = Peripherals::take()?;
    let menu_sel = PinDriver::input(p.pins.gpio3, Pull::Up)?;
    let menu_r1 = PinDriver::input(p.pins.gpio2, Pull::Up)?;
    let menu_r2 = PinDriver::input(p.pins.gpio1, Pull::Up)?;

    // OLED initialization
    let i2c_config = I2cConfig::new().baudrate(50_u32.kHz().into());
    let i2c = I2cDriver::new(p.i2c0, p.pins.gpio8, p.pins.gpio9, &i2c_config)?;
    let mut disp: Sh1106GM<_> = Sh1106Builder::new().connect_i2c(i2c).into();
    disp.init()
        .map_err(|e| anyhow::anyhow!("OLED init failed: {:?}", e))?;

    let hw_rev = global::get_hw_revision();
    let rotation = if hw_rev == 4 {
        sh1106::prelude::DisplayRotation::Rotate270
    } else {
        sh1106::prelude::DisplayRotation::Rotate90
    };
    disp.set_rotation(rotation).ok();
    disp.flush().ok();

    menu::draw_text(
        &mut disp,
        &format!(
            "Rusty\nHangul\nClock\nrev.{}\n\nno.{}\nver.{}\n\ninit...",
            hw_rev,
            nvs::get_device_no()?,
            global::get_sw_version()
        ),
    )?;

    // SPI & LEDs initialization
    let mut spi_driver = SpiDriver::new(
        p.spi2,
        p.pins.gpio4,
        p.pins.gpio6,
        AnyIOPin::none(),
        &SpiDriverConfig::new(),
    )?;
    let spi_config = SpiConfig::new()
        .baudrate(3200_u32.kHz().into())
        .data_mode(SPI_MODE);
    let spi_bus = SpiBusDriver::new(&mut spi_driver, &spi_config)?;
    let mut sleds = panel::Sleds::new(spi_bus);

    let sys_loop = EspSystemEventLoop::take()?;
    let timer_service = EspTaskTimerService::new()?;
    let nvs_partition = EspDefaultNvsPartition::take()?;
    let mut wifi = AsyncWifi::wrap(
        EspWifi::new(p.modem, sys_loop.clone(), Some(nvs_partition))?,
        sys_loop,
        timer_service,
    )?;

    futures::executor::block_on(async {
        inc_boot_count().await?;
        sleds.welcome().await;

        info!("Starting tasks...");
        global::register_task_with_wdt("main");
        global::start_stall_watchdog();

        let net_task = net::net_loop(&mut wifi);
        let show_time_task = show_time_loop(&mut sleds);
        let menu_task = menu::menu_loop(&mut disp, menu_sel);
        let time_sync_task = time_sync_loop();
        let rotary_encoder_task = rotary::rotary_encoder_loop(menu_r2, menu_r1);

        match futures::try_join!(
            menu_task,
            net_task,
            time_sync_task,
            show_time_task,
            rotary_encoder_task
        ) {
            Ok(_) => info!("All tasks completed"),
            Err(e) => warn!("Task error: {:?}", e),
        }
        Ok::<(), anyhow::Error>(())
    })?;

    info!("Restarting...");
    restart();

    // Ok(())
}

fn setup_panic_hook() {
    std::panic::set_hook(Box::new(|panic_info| {
        log::error!("Panic: {:?}", panic_info);

        let in_special_mode = *global::AP_MODE.lock().unwrap() || *global::OTA_MODE.lock().unwrap();
        if in_special_mode {
            warn!("Skipping restart in AP/OTA mode");
        } else {
            warn!("Restarting in 3 seconds...");
            restart();
        }
    }));
}

async fn time_sync_loop() -> anyhow::Result<()> {
    let mut watchdog = global::WatchdogManager::new(global::TaskId::TimeSync, 60, 10);
    let mut sync_check_cnt: u64 = 0;

    loop {
        timer::sleep_secs(60).await;
        if watchdog.update() {
            global::yield_to_other_tasks().await;
        }
        if net::check_net_cmd_or_skip().await.is_err() {
            continue;
        }

        // Sync every 3 days
        sync_check_cnt = (sync_check_cnt + 1) % (60 * 24 * 3);
        if sync_check_cnt == 0 {
            info!("Starting periodic time sync");
            if net::set_net_cmd("NTP") {
                let start = std::time::Instant::now();
                while start.elapsed().as_secs() < 60 {
                    global::heartbeat(global::TaskId::TimeSync);
                    let res = net::get_result_net();
                    if res == "OK" || res == "NG" {
                        break;
                    }
                    timer::sleep_secs(5).await;
                }
                net::set_result_net("");
            }
        }
    }
}

async fn inc_boot_count() -> anyhow::Result<()> {
    let count = nvs::get_boot_count()?.saturating_add(1);
    nvs::set_boot_count(count)
}

async fn show_time_loop<SPI: embedded_hal::spi::SpiBus>(
    sleds: &mut panel::Sleds<SPI>,
) -> anyhow::Result<()> {
    let mut watchdog = global::WatchdogManager::new(global::TaskId::ShowTime, 60, 10);
    let mut last_h = 255;
    let mut last_m = 255;
    let utc_offset = i64::from(nvs::get_utc_offset().unwrap_or(9));

    loop {
        if watchdog.update() {
            global::yield_to_other_tasks().await;
        }
        if net::check_net_cmd_or_skip().await.is_err() {
            timer::sleep_secs(1).await;
            continue;
        }

        let skip_display =
            *global::IN_MENU.lock().unwrap() || !*global::TIME_SYNCED.lock().unwrap();
        if skip_display {
            sleds.turn_on_all().await;
            last_h = 255;
            last_m = 255;
            timer::sleep_secs(1).await;
            continue;
        }

        let now = chrono::Utc::now() + chrono::Duration::hours(utc_offset);
        let h = now.hour() as u8;
        let m = now.minute() as u8;

        if h != last_h || m != last_m {
            last_h = h;
            last_m = m;
            {
                if let Ok(mut gh) = global::CUR_H.try_lock() {
                    *gh = h;
                }
                if let Ok(mut gm) = global::CUR_M.try_lock() {
                    *gm = m;
                }
            }
            debug!("Updating time: {:02}:{:02}", h, m);
            sleds.show_time(h, m).await;
        }
        timer::sleep_secs(1).await;
    }
}
