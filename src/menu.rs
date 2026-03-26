use std::time;

use esp_idf_svc::hal::i2c::*;
use esp_idf_svc::hal::reset::restart;
use log::info;
use sh1106::prelude::{GraphicsMode as Sh1106GM, I2cInterface};

use crate::{global, net, nvs, timer};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
        use MenuOption::*;
        match self {
            Ota => LedHue,
            LedHue => LedSat,
            LedSat => LedVal,
            LedVal => UtcOffset,
            UtcOffset => ApMode,
            ApMode => Wps,
            Wps => Ntp,
            Ntp => Exit,
            Exit => Ota,
        }
    }

    fn prev(&self) -> Self {
        use MenuOption::*;
        match self {
            Ota => Exit,
            LedHue => Ota,
            LedSat => LedHue,
            LedVal => LedSat,
            UtcOffset => LedVal,
            ApMode => UtcOffset,
            Wps => ApMode,
            Ntp => Wps,
            Exit => Ntp,
        }
    }

    fn index(&self) -> usize {
        *self as usize
    }

    const ALL: [Self; 9] = [
        Self::Ota,
        Self::LedHue,
        Self::LedSat,
        Self::LedVal,
        Self::UtcOffset,
        Self::ApMode,
        Self::Wps,
        Self::Ntp,
        Self::Exit,
    ];
}

pub async fn menu_loop(
    disp: &mut Sh1106GM<I2cInterface<I2cDriver<'_>>>,
    mut p_sel: impl embedded_hal::digital::InputPin,
) -> anyhow::Result<()> {
    info!("Starting menu_loop()...");

    let mut current_menu = MenuOption::Wps;
    let menu_len = MenuOption::ALL.len();
    let mut menu_enter_ts = get_ts();
    let mut sub_menu = false;
    let mut watchdog = global::WatchdogManager::new(200, 20);

    let owner_str = nvs::get_owner()
        .ok()
        .filter(|o| !o.is_empty())
        .map(|o| format!("\n\nfor\n{}", o))
        .unwrap_or_default();

    loop {
        timer::sleep_millis(50).await;

        if net::check_net_cmd_or_skip().await.is_err() {
            continue;
        }

        if watchdog.update() {
            global::yield_to_other_tasks().await;
        }

        let in_menu = *global::IN_MENU.lock().unwrap();

        if !in_menu {
            let h = *global::CUR_H.lock().unwrap();
            let m = *global::CUR_M.lock().unwrap();
            draw_text(
                disp,
                &format!("Rusty\nHangul\nClock{}\n\n{:02}:{:02}", owner_str, h, m),
            )?;

            if let Ok(mut event) = global::ROTARY_EVENT.try_lock() {
                if *event != global::RotaryEvent::None {
                    *event = global::RotaryEvent::None;
                    info!("Entering menu");
                    *global::IN_MENU.lock().unwrap() = true;
                    current_menu = MenuOption::Ota;
                    sub_menu = false;
                    menu_enter_ts = get_ts();
                }
            }
        } else {
            // Auto exit menu after 60 seconds
            if get_ts().saturating_sub(menu_enter_ts) > 60_000 {
                info!("Menu timeout, exiting");
                *global::IN_MENU.lock().unwrap() = false;
                continue;
            }

            if sub_menu {
                handle_sub_menu(
                    disp,
                    &mut current_menu,
                    &mut sub_menu,
                    &mut menu_enter_ts,
                    &mut p_sel,
                )
                .await?;
            } else {
                handle_main_menu(
                    disp,
                    &mut current_menu,
                    &mut sub_menu,
                    &mut menu_enter_ts,
                    &mut p_sel,
                    menu_len,
                )
                .await?;
            }
        }
    }
}

async fn handle_main_menu(
    disp: &mut Sh1106GM<I2cInterface<I2cDriver<'_>>>,
    current_menu: &mut MenuOption,
    sub_menu: &mut bool,
    menu_enter_ts: &mut u128,
    p_sel: &mut impl embedded_hal::digital::InputPin,
    menu_len: usize,
) -> anyhow::Result<()> {
    draw_text(
        disp,
        &format!(
            "=MENU {}/{}=\n\n{}\n\npress\nto\ndecide",
            current_menu.index() + 1,
            menu_len,
            current_menu.as_str()
        ),
    )?;

    if let Ok(mut event) = global::ROTARY_EVENT.try_lock() {
        match *event {
            global::RotaryEvent::Clockwise => {
                *current_menu = current_menu.next();
                *menu_enter_ts = get_ts();
            }
            global::RotaryEvent::CounterClockwise => {
                *current_menu = current_menu.prev();
                *menu_enter_ts = get_ts();
            }
            _ => {}
        }
        *event = global::RotaryEvent::None;
    }

    if p_sel.is_low().unwrap_or(false) {
        *menu_enter_ts = get_ts();
        match *current_menu {
            MenuOption::Exit => {
                *global::IN_MENU.lock().unwrap() = false;
                timer::sleep_secs(1).await;
            }
            MenuOption::LedHue
            | MenuOption::LedSat
            | MenuOption::LedVal
            | MenuOption::UtcOffset => {
                *sub_menu = true;
                timer::sleep_millis(200).await;
            }
            _ => {
                let cmd = current_menu.as_str();
                if net::set_net_cmd(cmd) {
                    draw_text(
                        disp,
                        &format!(
                            "MENU {}/{}\n\n**{}**\n\nwait...",
                            current_menu.index() + 1,
                            menu_len,
                            cmd
                        ),
                    )?;
                    let _ =
                        wait_for_net_result(disp, current_menu.index(), menu_len, cmd, 60).await;
                }
            }
        }
    }
    Ok(())
}

async fn handle_sub_menu(
    disp: &mut Sh1106GM<I2cInterface<I2cDriver<'_>>>,
    current_menu: &mut MenuOption,
    sub_menu: &mut bool,
    menu_enter_ts: &mut u128,
    p_sel: &mut impl embedded_hal::digital::InputPin,
) -> anyhow::Result<()> {
    let mut value: i16 = match current_menu {
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
        let step = match current_menu {
            MenuOption::UtcOffset => 1,
            _ => 5,
        };

        match *event {
            global::RotaryEvent::Clockwise => {
                value = (value + step).min(if matches!(current_menu, MenuOption::UtcOffset) {
                    12
                } else {
                    255
                });
                *menu_enter_ts = get_ts();
            }
            global::RotaryEvent::CounterClockwise => {
                value = (value - step).max(if matches!(current_menu, MenuOption::UtcOffset) {
                    -12
                } else {
                    0
                });
                *menu_enter_ts = get_ts();
            }
            _ => {}
        }
        *event = global::RotaryEvent::None;

        // Update global value immediately for feedback
        match current_menu {
            MenuOption::LedHue => *global::LED_HUE.lock().unwrap() = value as u8,
            MenuOption::LedSat => *global::LED_SAT.lock().unwrap() = value as u8,
            MenuOption::LedVal => *global::LED_VAL.lock().unwrap() = value as u8,
            MenuOption::UtcOffset => *global::UTC_OFFSET.lock().unwrap() = value as i8,
            _ => {}
        }
    }

    if p_sel.is_low().unwrap_or(false) {
        *sub_menu = false;
        timer::sleep_millis(200).await;
        match current_menu {
            MenuOption::LedHue | MenuOption::LedSat | MenuOption::LedVal => {
                let h = *global::LED_HUE.lock().unwrap();
                let s = *global::LED_SAT.lock().unwrap();
                let v = *global::LED_VAL.lock().unwrap();
                let _ = nvs::set_hsv(h, s, v);
            }
            MenuOption::UtcOffset => {
                let off = *global::UTC_OFFSET.lock().unwrap();
                let _ = nvs::set_utc_offset(off as i32);
                draw_text(disp, "REBOOTING...")?;
                timer::sleep_secs(1).await;
                restart();
            }
            _ => {}
        }
    }
    Ok(())
}

fn get_ts() -> u128 {
    time::SystemTime::now()
        .duration_since(time::UNIX_EPOCH)
        .unwrap()
        .as_millis()
}

async fn wait_for_net_result(
    disp: &mut Sh1106GM<I2cInterface<I2cDriver<'_>>>,
    menu_index: usize,
    menu_len: usize,
    menu_name: &str,
    timeout_secs: u32,
) -> anyhow::Result<String> {
    let start = get_ts();
    let timeout_ms = timeout_secs as u128 * 1000;

    loop {
        timer::sleep_millis(500).await;

        if get_ts().saturating_sub(start) > timeout_ms {
            draw_text(
                disp,
                &format!(
                    "MENU {}/{}\n\n{}\n**TIMEOUT**",
                    menu_index + 1,
                    menu_len,
                    menu_name
                ),
            )?;
            timer::sleep_secs(2).await;
            *global::IN_MENU.lock().unwrap() = false;
            return Err(anyhow::anyhow!("Timeout"));
        }

        let result = net::get_result_net();
        if result == "OK" || result == "NG" {
            draw_text(
                disp,
                &format!(
                    "MENU {}/{}\n\n{}\n\n**{}**",
                    menu_index + 1,
                    menu_len,
                    menu_name,
                    result
                ),
            )?;
            timer::sleep_secs(1).await;
            *global::IN_MENU.lock().unwrap() = false;
            return Ok(result);
        } else if !result.is_empty() {
            draw_text(
                disp,
                &format!(
                    "MENU {}/{}\n\n{}\n\n{}",
                    menu_index + 1,
                    menu_len,
                    menu_name,
                    result
                ),
            )?;
        }
    }
}

pub fn draw_text(disp: &mut Sh1106GM<I2cInterface<I2cDriver>>, text: &str) -> anyhow::Result<()> {
    use std::sync::Mutex;

    use embedded_graphics::mono_font::ascii::FONT_6X13;
    use embedded_graphics::mono_font::MonoTextStyleBuilder;
    use embedded_graphics::pixelcolor::BinaryColor;
    use embedded_graphics::prelude::*;
    use embedded_graphics::text::{Alignment, Text};
    use once_cell::sync::Lazy;

    static LAST_TEXT: Lazy<Mutex<String>> = Lazy::new(|| Mutex::new(String::new()));

    if let Ok(mut last) = LAST_TEXT.try_lock() {
        if *last == text {
            return Ok(());
        }
        *last = text.to_string();
    }

    let text_style = MonoTextStyleBuilder::new()
        .font(&FONT_6X13)
        .text_color(BinaryColor::On)
        .background_color(BinaryColor::Off)
        .build();

    disp.clear();
    Text::with_alignment(text, Point::new(32, 10), text_style, Alignment::Center).draw(disp)?;
    let _ = disp.flush();
    Ok(())
}
