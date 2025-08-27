// use esp_idf_svc::wifi::{AsyncWifi, EspWifi};
use std::sync::{
    // mpsc::{self, Receiver, Sender},
    Mutex,
};
use std::time;

use lazy_static::lazy_static;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RotaryEvent {
    None,
    Clockwise,
    CounterClockwise,
}

lazy_static! {
    pub static ref TIME_SYNCED: Mutex<bool> = Mutex::new(false);
    pub static ref CMD_NET: Mutex<String> = Mutex::new(String::new());
    pub static ref RESULT_NET: Mutex<String> = Mutex::new(String::new());
    pub static ref IN_MENU: Mutex<bool> = Mutex::new(false);
    pub static ref ROTARY_EVENT: Mutex<RotaryEvent> = Mutex::new(RotaryEvent::None);
    pub static ref CUR_H: Mutex<u8> = Mutex::new(0);
    pub static ref CUR_M: Mutex<u8> = Mutex::new(0);
    pub static ref LED_HUE: Mutex<u8> = Mutex::new(0);
    pub static ref LED_SAT: Mutex<u8> = Mutex::new(255);
    pub static ref LED_VAL: Mutex<u8> = Mutex::new(255);
    pub static ref UTC_OFFSET: Mutex<i8> = Mutex::new(9); // Default to KST (UTC+9)
    pub static ref BOOT_TIME: Mutex<u128> = Mutex::new(0);
}

pub fn get_uptime() -> u128 {
    let now = time::SystemTime::now();
    let timestamp = match now.duration_since(time::UNIX_EPOCH) {
        Ok(duration) => duration.as_millis(),
        Err(_) => {
            // 시스템 시간 오류 시 0 반환
            return 0;
        }
    };

    let boot_time = match BOOT_TIME.lock() {
        Ok(guard) => *guard,
        Err(_) => {
            // 락 오류 시 0 반환
            return 0;
        }
    };

    match boot_time {
        0 => {
            // 부트 시간이 설정되지 않은 경우에만 설정
            if let Ok(mut guard) = BOOT_TIME.lock() {
                *guard = timestamp;
            }
            0
        }
        _ => {
            // 오버플로우 방지를 위한 안전한 계산
            if timestamp >= boot_time {
                timestamp - boot_time
            } else {
                // 오버플로우 발생 시 0 반환
                0
            }
        }
    }
}

pub fn get_sw_version() -> i32 {
    match option_env!("RUSTY_HANGULCLOCK_SW_VERSION") {
        Some(s) => match s.parse::<i32>() {
            Ok(v) => v,
            Err(_) => 0,
        },
        None => 0,
    }
}

pub fn get_hw_revision() -> i32 {
    match option_env!("RUSTY_HANGULCLOCK_HW_REVISION") {
        Some(s) => match s.parse::<i32>() {
            Ok(v) => v,
            Err(_) => 0,
        },
        None => 0,
    }
}
