// use esp_idf_svc::wifi::{AsyncWifi, EspWifi};
use std::sync::{
    // mpsc::{self, Receiver, Sender},
    Mutex,
};
use std::time;

use lazy_static::lazy_static;

use crate::timer;

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
    pub static ref AP_MODE: Mutex<bool> = Mutex::new(false);
    pub static ref OTA_MODE: Mutex<bool> = Mutex::new(false);
}

pub fn get_uptime() -> u128 {
    let now = time::SystemTime::now();
    let timestamp = match now.duration_since(time::UNIX_EPOCH) {
        Ok(duration) => duration.as_secs() as u128,
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
            timestamp.saturating_sub(boot_time)
        }
    }
}

pub fn get_sw_version() -> i32 {
    option_env!("RUSTY_HANGULCLOCK_SW_VERSION")
        .unwrap_or_default()
        .parse::<i32>()
        .unwrap_or_default()
}

pub fn get_hw_revision() -> i32 {
    option_env!("RUSTY_HANGULCLOCK_HW_REVISION")
        .unwrap_or_default()
        .parse::<i32>()
        .unwrap_or_default()
}

/// Reset the task watchdog timer to prevent timeouts.
/// Note: In the current architecture, all async loops run within the same
/// FreeRTOS task (main), so this resets the main task's watchdog timer. Only
/// effective if CONFIG_ESP_TASK_WDT=y. Optimized for single-core ESP32C3
pub fn reset_task_watchdog() {
    // WDT is disabled
    /*
    #[cfg(target_os = "espidf")]
    unsafe {
        // Single-core: directly reset without checking task handle for better
        // performance Only reset if current task is registered with TWDT
        let result = esp_idf_svc::sys::esp_task_wdt_reset();
        if result != 0 {
            // Task not found or not registered - this is expected for some
            // tasks Don't log as error to avoid spam
        }
    }
    */
}

/// Yield control to other tasks briefly
/// Optimized for single-core ESP32C3: uses async sleep to yield to other async
/// futures
pub async fn yield_to_other_tasks() {
    reset_task_watchdog();

    // Use async sleep for both ESP-IDF and non-ESP-IDF to properly yield to other
    // async futures. vTaskDelay() is synchronous and doesn't create an await point
    // that allows the async runtime to switch between futures.
    timer::sleep_millis(1).await;
}

/// Safely register current task with Task Watchdog Timer
/// Returns true if registration succeeded or is not applicable
/// Note: Only effective when CONFIG_ESP_TASK_WDT=y
pub fn register_task_with_wdt(task_name: &str) -> bool {
    #[cfg(target_os = "espidf")]
    unsafe {
        let task_handle = esp_idf_svc::sys::xTaskGetCurrentTaskHandle();
        if task_handle.is_null() {
            log::warn!("{task_name}: Failed to get task handle for WDT registration");
            return false;
        }

        let result = esp_idf_svc::sys::esp_task_wdt_add(task_handle);
        if result == 0 {
            log::info!("{task_name}: Registered with TWDT");
            true
        } else {
            log::warn!("{task_name}: Failed to register with TWDT (error: {result})");
            false
        }
    }

    #[cfg(not(target_os = "espidf"))]
    {
        log::info!("{task_name}: WDT registration skipped (not ESP-IDF)");
        true
    }
}

/// Safely unregister current task from Task Watchdog Timer
#[allow(dead_code)]
pub fn unregister_task_from_wdt(task_name: &str) {
    /*
    #[cfg(target_os = "espidf")]
    unsafe {
        let task_handle = esp_idf_svc::sys::xTaskGetCurrentTaskHandle();
        if task_handle.is_null() {
            log::warn!("{task_name}: Failed to get task handle for WDT unregistration");
            return;
        }

        let result = esp_idf_svc::sys::esp_task_wdt_delete(task_handle);
        if result == 0 {
            log::info!("{task_name}: Unregistered from TWDT");
        } else {
            log::warn!("{task_name}: Failed to unregister from TWDT (error: {result})");
        }
    }

    #[cfg(not(target_os = "espidf"))]
    */
    {
        log::info!("{task_name}: WDT unregistration skipped (not ESP-IDF)");
    }
}

/// Watchdog manager helper to reduce code duplication
pub struct WatchdogManager {
    counter: u32,
    interval: u32,
    yield_interval: u32,
}

impl WatchdogManager {
    /// Create a new WatchdogManager with specified intervals
    pub fn new(watchdog_interval: u32, yield_interval: u32) -> Self {
        Self {
            counter: 0,
            interval: watchdog_interval,
            yield_interval,
        }
    }

    /// Update watchdog counter and reset if needed. Returns true if yield is
    /// recommended.
    pub fn update(&mut self) -> bool {
        // Watchdog 체크 - 오버플로우 방지를 위해 비교 후 증가
        if self.counter >= self.interval {
            log::debug!("Watchdog reset");
            self.counter = 0; // 명시적 리셋
            reset_task_watchdog();
        } else {
            self.counter += 1;
        }

        // Yield 권장 여부 반환
        self.yield_interval > 0 && self.counter % self.yield_interval == 0
    }

    /// Reset the counter manually
    #[allow(dead_code)]
    pub fn reset(&mut self) {
        self.counter = 0;
    }
}
