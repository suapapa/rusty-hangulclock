// use embassy_time::{Duration, Ticker, Timer};
use log::{debug, info, warn};
use rotary_encoder_hal::{Direction, Rotary};

use crate::{global, net, timer};
// use crate::panel::LED_WRITE_LOCK;

pub async fn rotary_encoder_loop(
    menu_r1: impl embedded_hal::digital::InputPin,
    menu_r2: impl embedded_hal::digital::InputPin,
) -> anyhow::Result<()> {
    info!("Starting rotary_encoder_loop()...");

    // Register with Task Watchdog Timer
    let _wdt_registered = global::register_task_with_wdt("rotary_encoder_loop");

    let mut enc = Rotary::new(menu_r1, menu_r2);
    // let mut ticker = Ticker::every(Duration::from_millis(10));
    let mut last_direction = Direction::None;
    let mut debounce_count = 0;
    const DEBOUNCE_THRESHOLD: u8 = 3; // Reduced threshold

    // Watchdog 카운터 추가
    let mut watchdog_counter = 0;
    const WATCHDOG_INTERVAL: u32 = 1000; // 10ms * 1000 = 10초마다 체크

    loop {
        match net::get_net_cmd() {
            Ok(cmd) => {
                if !cmd.is_empty() {
                    debug!("skip rotary encoder loop due to net cmd: {cmd}");
                    timer::sleep_millis(50).await;
                    continue;
                }
            }
            Err(e) => {
                warn!("Failed to get net cmd: {e}");
                timer::sleep_millis(50).await;
                continue;
            }
        }

        // Watchdog 체크
        watchdog_counter += 1;
        if watchdog_counter >= WATCHDOG_INTERVAL {
            debug!("Rotary encoder loop watchdog reset");
            watchdog_counter = 0;
            global::reset_task_watchdog();
        }

        // Yield to other tasks periodically
        if watchdog_counter % 100 == 0 {
            global::yield_to_other_tasks().await;
        }

        {
            match enc.update() {
                Ok(direction) => {
                    match direction {
                        Direction::Clockwise => {
                            // let _read_guard = LED_WRITE_LOCK.read().unwrap();
                            if last_direction != Direction::Clockwise {
                                debounce_count = 0;
                                last_direction = Direction::Clockwise;
                            }
                            debounce_count += 1;
                            if debounce_count >= DEBOUNCE_THRESHOLD {
                                info!("Clockwise");
                                if let Ok(mut event) = global::ROTARY_EVENT.try_lock() {
                                    *event = global::RotaryEvent::Clockwise;
                                } else {
                                    warn!("Failed to update rotary event (clockwise)");
                                }
                                debounce_count = 0;
                            }
                        }
                        Direction::CounterClockwise => {
                            // let _read_guard = LED_WRITE_LOCK.read().unwrap();
                            if last_direction != Direction::CounterClockwise {
                                debounce_count = 0;
                                last_direction = Direction::CounterClockwise;
                            }
                            debounce_count += 1;
                            if debounce_count >= DEBOUNCE_THRESHOLD {
                                info!("CounterClockwise");
                                if let Ok(mut event) = global::ROTARY_EVENT.try_lock() {
                                    *event = global::RotaryEvent::CounterClockwise;
                                } else {
                                    warn!("Failed to update rotary event (counter-clockwise)");
                                }
                                debounce_count = 0;
                            }
                        }
                        _ => {
                            // last_direction = Direction::None;
                            // debounce_count = 0;
                            // if let Ok(mut event) =
                            // global::ROTARY_EVENT.try_lock() {
                            //     *event = global::RotaryEvent::None;
                            // }
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to update rotary encoder: {e:?}");
                    // 에러 발생 시 짧은 대기
                    timer::sleep_millis(50).await;
                }
            }
        }
        timer::sleep_millis(10).await;
        // ticker.next().await;
    }
}
