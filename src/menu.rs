use std::time;

use esp_idf_svc::hal::i2c::*;
use esp_idf_svc::hal::reset::restart;
use log::{debug, info, warn};
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

    // Register with Task Watchdog Timer
    let _wdt_registered = global::register_task_with_wdt("menu_loop");

    let mut current_menu = MenuOption::Wps;
    let menu_options = MenuOption::all();
    let menu_len = menu_options.len();

    let mut menu_enter_ts: u128 = get_ts();
    let mut sub_menu = false;

    // Watchdog 카운터 추가
    let mut watchdog_counter = 0;
    const WATCHDOG_INTERVAL: u32 = 200; // 50ms * 200 = 10초마다 체크

    loop {
        timer::sleep_millis(50).await;

        // Watchdog 체크
        watchdog_counter += 1;
        if watchdog_counter >= WATCHDOG_INTERVAL {
            debug!("Menu loop watchdog reset");
            watchdog_counter = 0;
            global::reset_task_watchdog();
        }

        // Yield to other tasks periodically
        if watchdog_counter % 20 == 0 {
            global::yield_to_other_tasks().await;
        }
        let in_menu = {
            let in_menu = global::IN_MENU.lock().unwrap();
            *in_menu
        };

        if !in_menu {
            // let _read_guard = LED_WRITE_LOCK.read().unwrap();
            let h = *global::CUR_H.lock().unwrap();
            let m = *global::CUR_M.lock().unwrap();
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
                        {
                            let mut in_menu = global::IN_MENU.lock().unwrap();
                            *in_menu = true;
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
                {
                    let mut in_menu = global::IN_MENU.lock().unwrap();
                    *in_menu = false;
                }
                info!("exit menu");
                continue;
            }

            // let _read_guard = LED_WRITE_LOCK.read().unwrap();
            if sub_menu {
                let mut value = match current_menu {
                    MenuOption::LedHue => *global::LED_HUE.lock().unwrap() as i16,
                    MenuOption::LedSat => *global::LED_SAT.lock().unwrap() as i16,
                    MenuOption::LedVal => *global::LED_VAL.lock().unwrap() as i16,
                    MenuOption::UtcOffset => *global::UTC_OFFSET.lock().unwrap() as i16,
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
                        MenuOption::LedHue => *global::LED_HUE.lock().unwrap() = value as u8,
                        MenuOption::LedSat => *global::LED_SAT.lock().unwrap() = value as u8,
                        MenuOption::LedVal => *global::LED_VAL.lock().unwrap() = value as u8,
                        MenuOption::UtcOffset => *global::UTC_OFFSET.lock().unwrap() = value as i8,
                        _ => {}
                    }
                }

                if p_sel.is_low().unwrap() {
                    sub_menu = false;
                    timer::sleep_millis(200).await;
                    match current_menu {
                        MenuOption::LedHue | MenuOption::LedSat | MenuOption::LedVal => {
                            let hue = *global::LED_HUE.lock().unwrap();
                            let sat = *global::LED_SAT.lock().unwrap();
                            let val = *global::LED_VAL.lock().unwrap();
                            nvs::set_hsv(hue, sat, val).unwrap();
                        }
                        MenuOption::UtcOffset => {
                            let offset = *global::UTC_OFFSET.lock().unwrap();
                            nvs::set_utc_offset(offset as i32).unwrap();

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

                                        loop {
                                            timer::sleep_millis(1000).await;

                                            let result = net::get_result_net();
                                            if result.as_str() == "OK" || result.as_str() == "NG" {
                                                info!("NTP cmd completed");
                                                draw_text(
                                                    disp,
                                                    &format!(
                                                        "MENU {}/{}\n\nNTP\n**{}**",
                                                        current_menu.index() + 1,
                                                        menu_len,
                                                        result.as_str(),
                                                    ),
                                                )?;
                                                net::set_result_net("");
                                                timer::sleep_millis(1000).await;
                                                {
                                                    let mut in_menu =
                                                        global::IN_MENU.lock().unwrap();
                                                    *in_menu = false;
                                                }
                                                break;
                                            }
                                        }
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
                                        loop {
                                            timer::sleep_secs(1).await;

                                            let result = net::get_result_net();
                                            if result.as_str() == "OK" || result.as_str() == "NG" {
                                                info!("WPS cmd completed");
                                                draw_text(
                                                    disp,
                                                    &format!(
                                                        "MENU {}/{}\n\nWPS\n**{}**",
                                                        current_menu.index() + 1,
                                                        menu_len,
                                                        result.as_str(),
                                                    ),
                                                )?;
                                                net::set_result_net("");
                                                timer::sleep_secs(1).await;
                                                {
                                                    let mut in_menu =
                                                        global::IN_MENU.lock().unwrap();
                                                    *in_menu = false;
                                                }
                                                break;
                                            }
                                        }
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
                                        loop {
                                            timer::sleep_millis(10).await;

                                            let result = net::get_result_net();
                                            if result.as_str() == "OK" || result.as_str() == "NG" {
                                                info!("OTA cmd completed");
                                                draw_text(
                                                    disp,
                                                    &format!(
                                                        "MENU {}/{}\n\nOTA\n**{}**",
                                                        current_menu.index() + 1,
                                                        menu_len,
                                                        result.as_str(),
                                                    ),
                                                )?;
                                                net::set_result_net("");
                                                timer::sleep_secs(1).await;
                                                {
                                                    let mut in_menu =
                                                        global::IN_MENU.lock().unwrap();
                                                    *in_menu = false;
                                                }
                                                break;
                                            } else if result.as_str() != "" {
                                                draw_text(
                                                    disp,
                                                    &format!(
                                                        "MENU {}/{}\n\nOTA\n\nFLASHING\n\n{}",
                                                        current_menu.index() + 1,
                                                        menu_len,
                                                        result.as_str(),
                                                    ),
                                                )?;
                                            }
                                        }
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
                                        {
                                            let mut in_menu = global::IN_MENU.lock().unwrap();
                                            *in_menu = false;
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
    let mut last_text = LAST_TEXT.lock().unwrap();
    if *last_text == text {
        // 같으면 아무것도 하지 않음
        return Ok(());
    }
    // 다르면 업데이트
    *last_text = text.to_string();

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
