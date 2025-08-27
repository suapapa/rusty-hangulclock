use embassy_time::{Duration, Ticker, Timer};
use log::{info, warn};
use rotary_encoder_hal::{Direction, Rotary};

use crate::global;
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

    // Watchdog 카운터 추가
    let mut watchdog_counter = 0;
    const WATCHDOG_INTERVAL: u32 = 10000; // 10ms * 10000 = 100초마다 체크

    loop {
        // Watchdog 체크
        watchdog_counter += 1;
        if watchdog_counter >= WATCHDOG_INTERVAL {
            info!("Rotary encoder loop watchdog reset");
            watchdog_counter = 0;
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
                    Timer::after(Duration::from_millis(50)).await;
                }
            }
        }
        ticker.next().await;
    }
}
