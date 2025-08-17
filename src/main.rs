mod global;
mod menu;
mod net;
mod nvs;
mod ota_update;
mod panel;
mod report;
mod rotary;

use chrono::prelude::*;
use embassy_time::{Duration, Timer};
// use embedded_hal::spi::MODE_3;
use esp_idf_svc::hal::gpio::*;
use esp_idf_svc::hal::i2c::*;
use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::hal::prelude::*;
use esp_idf_svc::hal::spi::{
    config::Config as SpiConfig, config::DriverConfig as SpiDriverConfig, SpiBusDriver, SpiDriver,
};
use esp_idf_svc::hal::task;
use esp_idf_svc::timer::EspTaskTimerService;
use esp_idf_svc::wifi::{AsyncWifi, EspWifi};
use esp_idf_svc::{eventloop::EspSystemEventLoop, nvs::EspDefaultNvsPartition};
use log::{debug, info, warn};
use sh1106::{prelude::GraphicsMode as Sh1106GM, Builder as Sh1106Builder};
// use smart_leds::{gamma, hsv::hsv2rgb, hsv::Hsv, SmartLedsWrite, RGB8};
use std::time;

#[cfg(feature = "dotstar")]
use apa102_spi::MODE as SPI_MODE;
#[cfg(feature = "neopixel")]
use ws2812_spi::MODE as SPI_MODE;

fn main() -> anyhow::Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();
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

    // let mut disp_res = PinDriver::output(p_oled_res)?;
    // disp_res.set_low().unwrap();
    // std::thread::sleep(time::Duration::from_millis(100));
    // // Timer::after(Duration::from_millis(100)).await;
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
    disp.set_rotation(sh1106::prelude::DisplayRotation::Rotate270)
        .unwrap();
    disp.flush().unwrap();
    menu::draw_text(
        &mut disp,
        &format!(
            "Rusty\nHangul\nClock\nrev{}\nno.{}\n\ninit\n...",
            global::get_hw_revision(),
            nvs::get_device_no()?
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

    task::block_on(async {
        inc_boot_count().await?;
        Ok::<(), anyhow::Error>(())
    })?;

    // task::block_on(async {
    //     info!("initial time sync...");
    //     match net::sync_time_with_wifi(&mut wifi).await {
    //         Ok(_) => (),
    //         Err(e) => {
    //             warn!("Failed to sync time: {:?}", e);
    //         }
    //     }
    // });

    let net_task = net::net_loop(&mut wifi);
    let show_time_task = show_time_loop(&mut sleds);
    let menu_task = menu::menu_loop(&mut disp, menu_sel);
    let time_sync_task = time_sync_loop();
    let rotary_encoder_task = rotary::rotary_encoder_loop(menu_r2, menu_r1);

    info!("Starting tasks...");
    task::block_on(async {
        match futures::try_join!(
            menu_task,
            net_task,
            time_sync_task,
            show_time_task,
            rotary_encoder_task,
        ) {
            Ok(_) => info!("All tasks completed"),
            Err(e) => info!("Error in task: {:?}", e),
        }
    });

    Ok(())
}

async fn time_sync_loop() -> anyhow::Result<()> {
    info!("Starting time_sync_loop()...");

    let mut last_sync_time = time::SystemTime::now();
    let sync_interval = Duration::from_secs(60 * 60); // 1 hour

    // Watchdog 카운터 추가
    let mut watchdog_counter = 0;
    const WATCHDOG_INTERVAL: u32 = 3600; // 1시간마다 체크

    loop {
        let now = time::SystemTime::now();
        let duration = now.duration_since(last_sync_time).unwrap();

        // Watchdog 체크
        watchdog_counter += 1;
        if watchdog_counter >= WATCHDOG_INTERVAL {
            info!("Time sync loop watchdog reset");
            watchdog_counter = 0;
        }

        // TBD : 2 hours -> 2 days
        if duration.as_secs() > 60 * 60 * 2 {
            last_sync_time = now;
            info!("Syncing time...");
            {
                match global::CMD_NET.try_lock() {
                    Ok(mut cmd_net) => {
                        *cmd_net = "NTP".to_string();
                        info!("NTP cmd sent");
                    }
                    Err(_) => {
                        warn!("CMD_NET in use");
                    }
                }
            }

            // NTP 동기화 루프에 타임아웃 추가
            let mut timeout_count = 0;
            const MAX_TIMEOUT: u8 = 30; // 30초 타임아웃

            loop {
                Timer::after(Duration::from_secs(1)).await;
                timeout_count += 1;

                if timeout_count >= MAX_TIMEOUT {
                    warn!("NTP sync timeout, breaking loop");
                    break;
                }

                if let Ok(mut result) = global::RESULT_NET.try_lock() {
                    if result.as_str() == "OK" || result.as_str() == "NG" {
                        info!("NTP cmd completed: {}", result.as_str());
                        *result = "".to_string();
                        break;
                    }
                }
            }
        } else {
            Timer::after(sync_interval).await;
        }
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
    //sleds.turn_on_all();

    let mut skip_display: bool; // = false;
    let mut last_h: u8 = 0;
    let mut last_m: u8 = 0;

    // Watchdog 카운터 추가
    let mut watchdog_counter = 0;
    const WATCHDOG_INTERVAL: u32 = 3600; // 1시간마다 체크

    let utc_offset: i32 = nvs::get_utc_offset()?;

    loop {
        // Watchdog 체크
        watchdog_counter += 1;
        if watchdog_counter >= WATCHDOG_INTERVAL {
            info!("Show time loop watchdog reset");
            watchdog_counter = 0;
        }

        match global::IN_MENU.try_lock() {
            Ok(in_menu) => {
                skip_display = *in_menu;
            }
            Err(_) => {
                debug!("IN_MENU in use");
                Timer::after(Duration::from_secs(1)).await;
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
                Timer::after(Duration::from_secs(1)).await;
                continue;
            }
        }

        if skip_display {
            sleds.turn_on_all();
            last_h = 0;
            last_m = 0;
            Timer::after(Duration::from_secs(1)).await;
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
            debug!("Time updated, h: {}, m: {}", h, m);
            sleds.show_time(h, m);
        }
        Timer::after(Duration::from_secs(1)).await;
    }
    // Ok(())
}
