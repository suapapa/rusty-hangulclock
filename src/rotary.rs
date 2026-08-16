use embassy_time::{Duration, Ticker};
use log::{info, warn};
use rotary_encoder_hal::{Direction, Rotary};

use crate::{global, net};

pub async fn rotary_encoder_loop(
    r1: impl embedded_hal::digital::InputPin,
    r2: impl embedded_hal::digital::InputPin,
) -> anyhow::Result<()> {
    info!("Starting rotary_encoder_loop()...");

    let mut enc = Rotary::new(r1, r2);
    let mut ticker = Ticker::every(Duration::from_millis(10));
    let mut last_direction = Direction::None;
    let mut debounce_count = 0;
    const DEBOUNCE_THRESHOLD: u8 = 3;

    let mut watchdog = global::WatchdogManager::new(global::TaskId::Rotary, 1000, 100);

    loop {
        if watchdog.update() {
            global::yield_to_other_tasks().await;
        }

        if net::check_net_cmd_or_skip().await.is_err() {
            ticker.next().await;
            continue;
        }

        match enc.update() {
            Ok(direction) if direction != Direction::None => {
                if last_direction != direction {
                    debounce_count = 0;
                    last_direction = direction;
                }
                
                debounce_count += 1;
                if debounce_count >= DEBOUNCE_THRESHOLD {
                    let event = match direction {
                        Direction::Clockwise => global::RotaryEvent::Clockwise,
                        Direction::CounterClockwise => global::RotaryEvent::CounterClockwise,
                        _ => global::RotaryEvent::None,
                    };

                    if event != global::RotaryEvent::None {
                        info!("Rotary event: {:?}", event);
                        if let Ok(mut global_event) = global::ROTARY_EVENT.try_lock() {
                            *global_event = event;
                        }
                    }
                    debounce_count = 0;
                }
            }
            Ok(_) => {
                // Direction::None, resetting last_direction might be too aggressive
                // but usually rotary encoders pulse.
            }
            Err(e) => {
                warn!("Rotary encoder error: {:?}", e);
            }
        }
        ticker.next().await;
    }
}
