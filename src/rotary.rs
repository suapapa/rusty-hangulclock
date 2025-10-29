use embassy_time::{Duration, Ticker};
use log::{info, warn};
use rotary_encoder_hal::{Direction, Rotary};

use crate::{global, net};
// use crate::panel::LED_WRITE_LOCK;

pub async fn rotary_encoder_loop(
    menu_r1: impl embedded_hal::digital::InputPin,
    menu_r2: impl embedded_hal::digital::InputPin,
) -> anyhow::Result<()> {
    info!("Starting rotary_encoder_loop()...");

    let mut enc = Rotary::new(menu_r1, menu_r2);
    let mut ticker = Ticker::every(Duration::from_millis(10));
    let mut last_direction = Direction::None;
    let mut debounce_count = 0;
    const DEBOUNCE_THRESHOLD: u8 = 3; // Reduced threshold

    // Watchdog manager (10ms * 1000 = 10초마다 체크, 100회마다 yield)
    let mut watchdog = global::WatchdogManager::new(1000, 100);

    loop {
        // 네트워크 명령 체크
        if net::check_net_cmd_or_skip().await.is_err() {
            ticker.next().await;
            continue;
        }

        // Watchdog 체크 및 yield
        if watchdog.update() {
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
                }
            }
        }
        ticker.next().await;
    }
}
