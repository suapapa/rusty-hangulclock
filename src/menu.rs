use std::time;

use esp_idf_svc::hal::i2c::*;
use esp_idf_svc::hal::reset::restart;
use log::{info, warn};
use sh1106::prelude::{GraphicsMode as Sh1106GM, I2cInterface};

use crate::{global, net, nvs, timer};

#[derive(Debug, Clone, Copy, PartialEq)]
enum MenuOption {
    Ota,
    LedHue,
    LedSat,
    LedVal,
    UtcOffset,
    ApMode,
    Wps,
    Ntp,
    Exit,
}

impl MenuOption {
    fn as_str(&self) -> &'static str {
        match self {
            MenuOption::Ota => "OTA",
            MenuOption::LedHue => "LED HUE",
            MenuOption::LedSat => "LED SAT",
            MenuOption::LedVal => "LED VAL",
            MenuOption::UtcOffset => "UTC OFFSET",
            MenuOption::ApMode => "AP MODE",
            MenuOption::Wps => "WPS",
            MenuOption::Ntp => "NTP",
            MenuOption::Exit => "EXIT",
        }
    }

    fn next(&self) -> Self {
        match self {
            MenuOption::Ota => MenuOption::LedHue,
            MenuOption::LedHue => MenuOption::LedSat,
            MenuOption::LedSat => MenuOption::LedVal,
            MenuOption::LedVal => MenuOption::UtcOffset,
            MenuOption::UtcOffset => MenuOption::ApMode,
            MenuOption::ApMode => MenuOption::Wps,
            MenuOption::Wps => MenuOption::Ntp,
            MenuOption::Ntp => MenuOption::Exit,
            MenuOption::Exit => MenuOption::Ota,
        }
    }

    fn prev(&self) -> Self {
        match self {
            MenuOption::Ota => MenuOption::Exit,
            MenuOption::LedHue => MenuOption::Ota,
            MenuOption::LedSat => MenuOption::LedHue,
            MenuOption::LedVal => MenuOption::LedSat,
            MenuOption::UtcOffset => MenuOption::LedVal,
            MenuOption::ApMode => MenuOption::UtcOffset,
            MenuOption::Wps => MenuOption::ApMode,
            MenuOption::Ntp => MenuOption::Wps,
            MenuOption::Exit => MenuOption::Ntp,
        }
    }

    fn all() -> [Self; 9] {
        [
            MenuOption::Ota,
            MenuOption::LedHue,
            MenuOption::LedSat,
            MenuOption::LedVal,
            MenuOption::UtcOffset,
            MenuOption::ApMode,
            MenuOption::Wps,
            MenuOption::Ntp,
            MenuOption::Exit,
        ]
    }

    fn index(&self) -> usize {
        match self {
            MenuOption::Ota => 0,
            MenuOption::LedHue => 1,
            MenuOption::LedSat => 2,
            MenuOption::LedVal => 3,
            MenuOption::UtcOffset => 4,
            MenuOption::ApMode => 5,
            MenuOption::Wps => 6,
            MenuOption::Ntp => 7,
            MenuOption::Exit => 8,
        }
    }
}

pub async fn menu_loop(
    disp: &mut Sh1106GM<I2cInterface<I2cDriver<'_>>>,
    mut p_sel: impl embedded_hal::digital::InputPin + embedded_hal_async::digital::Wait,
) -> anyhow::Result<()> {
    info!("Starting menu_loop()...");

    let mut current_menu = MenuOption::Wps;
    let menu_options = MenuOption::all();
    let menu_len = menu_options.len();

    let mut menu_enter_ts: u128 = get_ts();
    let mut sub_menu = false;

    // Watchdog manager (50ms * 200 = 10초마다 체크, 20회마다 yield)
    let mut watchdog = global::WatchdogManager::new(200, 20);

    loop {
        timer::sleep_millis(50).await;

        // Watchdog 체크 및 yield
        if watchdog.update() {
            global::yield_to_other_tasks().await;
        }
        let in_menu = match global::IN_MENU.try_lock() {
            Ok(in_menu) => *in_menu,
            Err(_) => {
                // 다른 태스크가 락을 잡고 있으면 잠시 대기 후 재시도
                timer::sleep_millis(10).await;
                continue;
            }
        };

        if !in_menu {
            // let _read_guard = LED_WRITE_LOCK.read().unwrap();
            let (h, m) = match (global::CUR_H.try_lock(), global::CUR_M.try_lock()) {
                (Ok(h_guard), Ok(m_guard)) => (*h_guard, *m_guard),
                _ => {
                    // 락을 얻지 못하면 스킵하고 다음 루프로
                    timer::sleep_millis(10).await;
                    continue;
                }
            };
            let time_str = format!("{h:02}:{m:02}");
            let sw_ver_str = format!("sw-v{}", global::get_sw_version());

            draw_text(
                disp,
                &format!("Rusty\nHangul\nClock\n{sw_ver_str}\n\n{time_str}\n\nrotate\nknob"),
            )?;
            if let Ok(mut event) = global::ROTARY_EVENT.try_lock() {
                match *event {
                    global::RotaryEvent::Clockwise | global::RotaryEvent::CounterClockwise => {
                        *event = global::RotaryEvent::None;
                        info!("enter menu");
                        match global::IN_MENU.try_lock() {
                            Ok(mut in_menu) => {
                                *in_menu = true;
                            }
                            Err(_) => {
                                warn!("Failed to lock IN_MENU");
                                timer::sleep_millis(10).await;
                                continue;
                            }
                        }
                        current_menu = MenuOption::Ota;
                        sub_menu = false;
                        menu_enter_ts = get_ts();
                    }
                    _ => {}
                }
            }
        } else {
            let ts_now = get_ts();
            if (ts_now - menu_enter_ts) > 60 * 1000 {
                match global::IN_MENU.try_lock() {
                    Ok(mut in_menu) => {
                        *in_menu = false;
                    }
                    Err(_) => {
                        warn!("Failed to unlock IN_MENU on timeout");
                    }
                }
                info!("exit menu");
                continue;
            }

            // let _read_guard = LED_WRITE_LOCK.read().unwrap();
            if sub_menu {
                let mut value = match current_menu {
                    MenuOption::LedHue => match global::LED_HUE.try_lock() {
                        Ok(hue) => *hue as i16,
                        Err(_) => {
                            warn!("Failed to lock LED_HUE");
                            0
                        }
                    },
                    MenuOption::LedSat => match global::LED_SAT.try_lock() {
                        Ok(sat) => *sat as i16,
                        Err(_) => {
                            warn!("Failed to lock LED_SAT");
                            0
                        }
                    },
                    MenuOption::LedVal => match global::LED_VAL.try_lock() {
                        Ok(val) => *val as i16,
                        Err(_) => {
                            warn!("Failed to lock LED_VAL");
                            0
                        }
                    },
                    MenuOption::UtcOffset => match global::UTC_OFFSET.try_lock() {
                        Ok(offset) => *offset as i16,
                        Err(_) => {
                            warn!("Failed to lock UTC_OFFSET");
                            0
                        }
                    },
                    _ => 0,
                };

                draw_text(
                    disp,
                    &format!(
                        "={}=\n\n{}\n\npress\nto\ndecide",
                        current_menu.as_str(),
                        value
                    ),
                )?;

                if let Ok(mut event) = global::ROTARY_EVENT.try_lock() {
                    match *event {
                        global::RotaryEvent::Clockwise => {
                            match current_menu {
                                MenuOption::LedHue | MenuOption::LedSat | MenuOption::LedVal => {
                                    value += 5;
                                    if value > 255 {
                                        value = 255;
                                    }
                                }
                                MenuOption::UtcOffset => {
                                    value += 1;
                                    if value > 12 {
                                        value = 12;
                                    }
                                }
                                _ => {}
                            }
                            menu_enter_ts = get_ts();
                            *event = global::RotaryEvent::None;
                        }
                        global::RotaryEvent::CounterClockwise => {
                            match current_menu {
                                MenuOption::LedHue | MenuOption::LedSat | MenuOption::LedVal => {
                                    value -= 5;
                                    if value < 0 {
                                        value = 0;
                                    }
                                }
                                MenuOption::UtcOffset => {
                                    value -= 1;
                                    if value < -12 {
                                        value = -12;
                                    }
                                }
                                _ => {}
                            }
                            menu_enter_ts = get_ts();
                            *event = global::RotaryEvent::None;
                        }
                        _ => {}
                    }

                    match current_menu {
                        MenuOption::LedHue => {
                            if let Ok(mut hue) = global::LED_HUE.try_lock() {
                                *hue = value as u8;
                            } else {
                                warn!("Failed to lock LED_HUE for update");
                            }
                        }
                        MenuOption::LedSat => {
                            if let Ok(mut sat) = global::LED_SAT.try_lock() {
                                *sat = value as u8;
                            } else {
                                warn!("Failed to lock LED_SAT for update");
                            }
                        }
                        MenuOption::LedVal => {
                            if let Ok(mut val) = global::LED_VAL.try_lock() {
                                *val = value as u8;
                            } else {
                                warn!("Failed to lock LED_VAL for update");
                            }
                        }
                        MenuOption::UtcOffset => {
                            if let Ok(mut offset) = global::UTC_OFFSET.try_lock() {
                                *offset = value as i8;
                            } else {
                                warn!("Failed to lock UTC_OFFSET for update");
                            }
                        }
                        _ => {}
                    }
                }

                if p_sel.is_low().unwrap() {
                    sub_menu = false;
                    timer::sleep_millis(200).await;
                    match current_menu {
                        MenuOption::LedHue | MenuOption::LedSat | MenuOption::LedVal => {
                            match (
                                global::LED_HUE.try_lock(),
                                global::LED_SAT.try_lock(),
                                global::LED_VAL.try_lock(),
                            ) {
                                (Ok(hue), Ok(sat), Ok(val)) => {
                                    nvs::set_hsv(*hue, *sat, *val).unwrap();
                                }
                                _ => {
                                    warn!("Failed to lock LED values for NVS save");
                                }
                            }
                        }
                        MenuOption::UtcOffset => {
                            match global::UTC_OFFSET.try_lock() {
                                Ok(offset) => {
                                    nvs::set_utc_offset(*offset as i32).unwrap();
                                }
                                Err(_) => {
                                    warn!("Failed to lock UTC_OFFSET for NVS save");
                                }
                            }

                            timer::sleep_millis(1000).await;
                            esp_idf_svc::hal::reset::restart();
                        }
                        _ => {}
                    }
                }
            } else {
                draw_text(
                    disp,
                    &format!(
                        "=MENU {}/{}=\n\n{}\n\npress\nto\ndecide",
                        current_menu.index() + 1,
                        menu_len,
                        current_menu.as_str()
                    ),
                )?;

                let (ok, event) = {
                    if let Ok(mut event) = global::ROTARY_EVENT.try_lock() {
                        let ret = *event;
                        *event = global::RotaryEvent::None;
                        (true, ret)
                    } else {
                        (false, global::RotaryEvent::None)
                    }
                };

                if ok {
                    match event {
                        global::RotaryEvent::Clockwise => {
                            current_menu = current_menu.next();
                            info!("Menu changed to: {current_menu:?}");
                            menu_enter_ts = get_ts();
                        }
                        global::RotaryEvent::CounterClockwise => {
                            current_menu = current_menu.prev();
                            info!("Menu changed to: {current_menu:?}");
                            menu_enter_ts = get_ts();
                        }
                        global::RotaryEvent::None => {
                            if p_sel.is_low().unwrap() {
                                info!("decide");
                                menu_enter_ts = get_ts();
                                match current_menu {
                                    MenuOption::Ntp => {
                                        info!("NTP selected");
                                        if !net::set_net_cmd("NTP") {
                                            warn!("Failed to send NTP cmd");
                                            timer::sleep_secs(1).await;
                                            continue;
                                        }
                                        draw_text(
                                            disp,
                                            &format!(
                                                "MENU {}/{}\n\n**NTP**\n\nwait\na\nmoment",
                                                current_menu.index() + 1,
                                                menu_len,
                                            ),
                                        )?;

                                        let _ = wait_for_net_result(
                                            disp,
                                            current_menu.index(),
                                            menu_len,
                                            "NTP",
                                            60,
                                            1000,
                                            false,
                                        )
                                        .await;
                                    }
                                    MenuOption::ApMode => {
                                        info!("AP MODE selected");
                                        if !net::set_net_cmd("AP") {
                                            warn!("Failed to send AP cmd");
                                            timer::sleep_secs(1).await;
                                            continue;
                                        }
                                        draw_text(
                                            disp,
                                            &format!(
                                                "MENU {}/{}\n\n**AP MODE**\n\nconnect\nto\n192.168\n.71.1\nfor\nconfig",
                                                current_menu.index() + 1,
                                                menu_len,
                                            ),
                                        )?;
                                        loop {
                                            timer::sleep_millis(100).await;
                                            // if press button, reboot
                                            if p_sel.is_low().unwrap() {
                                                info!("reboot");
                                                draw_text(
                                                    disp,
                                                    &format!(
                                                        "MENU {}/{}\n\nAP MODE\n\n**REBOOTING**",
                                                        current_menu.index() + 1,
                                                        menu_len,
                                                    ),
                                                )?;
                                                restart();
                                            }
                                        }
                                    }
                                    MenuOption::Wps => {
                                        info!("WPS selected");
                                        if !net::set_net_cmd("WPS") {
                                            warn!("Failed to send WPS cmd");
                                            timer::sleep_secs(1).await;
                                            continue;
                                        }
                                        draw_text(
                                            disp,
                                            &format!(
                                                "MENU {}/{}\n\n**WPS**\n\nwait\na\nmoment",
                                                current_menu.index() + 1,
                                                menu_len,
                                            ),
                                        )?;

                                        let _ = wait_for_net_result(
                                            disp,
                                            current_menu.index(),
                                            menu_len,
                                            "WPS",
                                            120,
                                            1000,
                                            false,
                                        )
                                        .await;
                                    }
                                    MenuOption::Ota => {
                                        info!("OTA selected");
                                        if !net::set_net_cmd("OTA") {
                                            warn!("Failed to send OTA cmd");
                                            timer::sleep_secs(1).await;
                                            continue;
                                        }
                                        draw_text(
                                            disp,
                                            &format!(
                                                "MENU {}/{}\n\n**OTA**\n\nwait\na\nmoment",
                                                current_menu.index() + 1,
                                                menu_len,
                                            ),
                                        )?;

                                        let _ = wait_for_net_result(
                                            disp,
                                            current_menu.index(),
                                            menu_len,
                                            "OTA",
                                            300,
                                            10,
                                            true,
                                        )
                                        .await;
                                    }
                                    MenuOption::LedHue
                                    | MenuOption::LedSat
                                    | MenuOption::LedVal => {
                                        // LED color settings
                                        sub_menu = true;
                                        timer::sleep_millis(200).await;
                                    }
                                    MenuOption::UtcOffset => {
                                        // UTC OFFSET
                                        sub_menu = true;
                                        timer::sleep_millis(200).await;
                                    }
                                    MenuOption::Exit => {
                                        // EXIT
                                        info!("EXIT selected");
                                        draw_text(
                                            disp,
                                            &format!(
                                                "MENU {}/{}\n\n**EXIT**",
                                                current_menu.index() + 1,
                                                menu_len,
                                            ),
                                        )?;
                                        timer::sleep_secs(1).await;
                                        match global::IN_MENU.try_lock() {
                                            Ok(mut in_menu) => {
                                                *in_menu = false;
                                            }
                                            Err(_) => {
                                                warn!("Failed to unlock IN_MENU on exit");
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn get_ts() -> u128 {
    let now = time::SystemTime::now();

    now.duration_since(time::UNIX_EPOCH).unwrap().as_millis()
}

/// Wait for network command result with timeout and optional flashing status
/// updates
async fn wait_for_net_result(
    disp: &mut Sh1106GM<I2cInterface<I2cDriver<'_>>>,
    menu_index: usize,
    menu_len: usize,
    menu_name: &str,
    max_timeout_secs: u32,
    sleep_interval_ms: u64,
    show_flashing: bool,
) -> anyhow::Result<String> {
    let mut timeout_count = 0u32;
    // Calculate max ticks: convert seconds to milliseconds, then divide by sleep
    // interval
    let max_timeout_ticks = (max_timeout_secs as u64 * 1000 / sleep_interval_ms) as u32;
    let mut last_flashing_update = 0u32;

    loop {
        timer::sleep_millis(sleep_interval_ms).await;
        timeout_count += 1;

        if timeout_count >= max_timeout_ticks {
            warn!("{menu_name} timeout after {} seconds", max_timeout_secs);
            draw_text(
                disp,
                &format!(
                    "MENU {}/{}\n\n{menu_name}\n**TIMEOUT**",
                    menu_index + 1,
                    menu_len,
                ),
            )?;
            net::set_result_net("");
            timer::sleep_secs(2).await;
            match global::IN_MENU.try_lock() {
                Ok(mut in_menu) => {
                    *in_menu = false;
                }
                Err(_) => {
                    warn!("Failed to unlock IN_MENU on timeout");
                }
            }
            return Err(anyhow::anyhow!("{menu_name} timeout"));
        }

        let result = net::get_result_net();
        let result_str = result.as_str();
        if result_str == "OK" || result_str == "NG" {
            info!("{menu_name} cmd completed");
            draw_text(
                disp,
                &format!(
                    "MENU {}/{}\n\n{menu_name}\n**{result_str}**",
                    menu_index + 1,
                    menu_len,
                ),
            )?;
            net::set_result_net("");
            timer::sleep_secs(1).await;
            match global::IN_MENU.try_lock() {
                Ok(mut in_menu) => {
                    *in_menu = false;
                }
                Err(_) => {
                    warn!("Failed to unlock IN_MENU");
                }
            }
            return Ok(result_str.to_string());
        } else if show_flashing && !result_str.is_empty() {
            // FLASHING 상태 업데이트는 주기적으로만 (과도한 화면 업데이트 방지)
            let flashing_update_interval = if sleep_interval_ms <= 100 { 100 } else { 10 };
            if timeout_count - last_flashing_update >= flashing_update_interval {
                draw_text(
                    disp,
                    &format!(
                        "MENU {}/{}\n\n{menu_name}\n\nFLASHING\n\n{result_str}",
                        menu_index + 1,
                        menu_len,
                    ),
                )?;
                last_flashing_update = timeout_count;
            }
        }
    }
}

use std::sync::Mutex;

use once_cell::sync::Lazy;

static LAST_TEXT: Lazy<Mutex<String>> = Lazy::new(|| Mutex::new(String::new()));

pub fn draw_text(disp: &mut Sh1106GM<I2cInterface<I2cDriver>>, text: &str) -> anyhow::Result<()> {
    use embedded_graphics::mono_font::ascii::FONT_6X13;
    use embedded_graphics::mono_font::MonoTextStyleBuilder;
    use embedded_graphics::pixelcolor::BinaryColor;
    use embedded_graphics::prelude::*;
    use embedded_graphics::text::{Alignment, Text};

    // Wait for any LED write operations to complete
    // let _read_guard = LED_WRITE_LOCK.read().unwrap();

    // last_text와 다를 때만 출력
    let should_update = match LAST_TEXT.try_lock() {
        Ok(mut last_text) => {
            if *last_text == text {
                // 같으면 아무것도 하지 않음
                return Ok(());
            }
            // 다르면 업데이트
            *last_text = text.to_string();
            true
        }
        Err(_) => {
            // 락을 얻지 못하면 일단 업데이트 수행 (중복 출력 방지는 포기)
            warn!("Failed to lock LAST_TEXT, proceeding with update anyway");
            true
        }
    };

    if !should_update {
        return Ok(());
    }

    let text_style = MonoTextStyleBuilder::new()
        .font(&FONT_6X13)
        .text_color(BinaryColor::On)
        .background_color(BinaryColor::Off)
        .build();

    disp.clear();
    Text::with_alignment(text, Point::new(64 / 2, 10), text_style, Alignment::Center).draw(disp)?;
    disp.flush().unwrap();
    Ok(())
}
