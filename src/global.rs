use std::sync::Mutex;
use std::time;

use once_cell::sync::Lazy;

use crate::timer;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RotaryEvent {
    None,
    Clockwise,
    CounterClockwise,
}

pub static TIME_SYNCED: Lazy<Mutex<bool>> = Lazy::new(|| Mutex::new(false));
pub static CMD_NET: Lazy<Mutex<String>> = Lazy::new(|| Mutex::new(String::new()));
pub static RESULT_NET: Lazy<Mutex<String>> = Lazy::new(|| Mutex::new(String::new()));
pub static IN_MENU: Lazy<Mutex<bool>> = Lazy::new(|| Mutex::new(false));
pub static ROTARY_EVENT: Lazy<Mutex<RotaryEvent>> = Lazy::new(|| Mutex::new(RotaryEvent::None));
pub static CUR_H: Lazy<Mutex<u8>> = Lazy::new(|| Mutex::new(0));
pub static CUR_M: Lazy<Mutex<u8>> = Lazy::new(|| Mutex::new(0));
pub static LED_HUE: Lazy<Mutex<u8>> = Lazy::new(|| Mutex::new(0));
pub static LED_SAT: Lazy<Mutex<u8>> = Lazy::new(|| Mutex::new(255));
pub static LED_VAL: Lazy<Mutex<u8>> = Lazy::new(|| Mutex::new(255));
pub static UTC_OFFSET: Lazy<Mutex<i8>> = Lazy::new(|| Mutex::new(9)); // Default to KST (UTC+9)
pub static BOOT_TIME: Lazy<Mutex<u128>> = Lazy::new(|| Mutex::new(0));
pub static AP_MODE: Lazy<Mutex<bool>> = Lazy::new(|| Mutex::new(false));
pub static OTA_MODE: Lazy<Mutex<bool>> = Lazy::new(|| Mutex::new(false));

pub fn get_uptime() -> u128 {
    let now = match time::SystemTime::now().duration_since(time::UNIX_EPOCH) {
        Ok(duration) => duration.as_secs() as u128,
        Err(_) => return 0,
    };

    let mut boot_time_guard = match BOOT_TIME.lock() {
        Ok(guard) => guard,
        Err(_) => return 0,
    };

    if *boot_time_guard == 0 {
        *boot_time_guard = now;
        0
    } else {
        now.saturating_sub(*boot_time_guard)
    }
}

pub fn get_sw_version() -> i32 {
    option_env!("RUSTY_HANGULCLOCK_SW_VERSION")
        .and_then(|v| v.parse::<i32>().ok())
        .unwrap_or_default()
}

pub fn get_hw_revision() -> i32 {
    option_env!("RUSTY_HANGULCLOCK_HW_REVISION")
        .and_then(|v| v.parse::<i32>().ok())
        .unwrap_or_default()
}

/// Reset the task watchdog timer to prevent timeouts.
/// Integrated with ESP-IDF's task watchdog if enabled.
pub fn reset_task_watchdog() {
    // TWDT was never initialized, so we can't reset it.
    /*
       #[cfg(target_os = "espidf")]
       unsafe {
           // Only attempt reset if we're on ESP-IDF
           let _ = esp_idf_svc::sys::esp_task_wdt_reset();
       }
    */
}

/// Yield control to other tasks briefly by sleeping for 1ms.
/// This allows the async executor to poll other futures.
pub async fn yield_to_other_tasks() {
    reset_task_watchdog();
    timer::sleep_millis(1).await;
}

/// Safely register current task with Task Watchdog Timer
/// Returns true if registration succeeded or is not applicable
pub fn register_task_with_wdt(_task_name: &str) -> bool {
    // TWDT was never initialized, so we can't reset it.
    /*
    #[cfg(target_os = "espidf")]
    {
        unsafe {
            let task_handle = esp_idf_svc::sys::xTaskGetCurrentTaskHandle();
            if task_handle.is_null() {
                log::warn!("{task_name}: Failed to get task handle for WDT registration");
                return false;
            }

            let result = esp_idf_svc::sys::esp_task_wdt_add(task_handle);
            if result == 0 {
                log::info!("{task_name}: Registered with TWDT");
                return true;
            } else {
                log::warn!("{task_name}: Failed to register with TWDT (error: {result})");
                return false;
            }
        }
    }
    */

    #[cfg(not(target_os = "espidf"))]
    {
        log::info!("{task_name}: WDT registration skipped (not ESP-IDF)");
        true
    }

    true
}

/// Safely unregister current task from Task Watchdog Timer
#[allow(dead_code)]
pub fn unregister_task_from_wdt(_task_name: &str) {
    // TWDT was never initialized, so we can't reset it.
    /*
    #[cfg(target_os = "espidf")]
    {
        unsafe {
            let task_handle = esp_idf_svc::sys::xTaskGetCurrentTaskHandle();
            if !task_handle.is_null() {
                let _ = esp_idf_svc::sys::esp_task_wdt_delete(task_handle);
            }
        }
    }
    */

    #[cfg(not(target_os = "espidf"))]
    {
        log::info!("{task_name}: WDT unregistration skipped (not ESP-IDF)");
    }
}

/// Watchdog manager helper to reduce code duplication and handle periodic
/// yielding.
pub struct WatchdogManager {
    counter: u32,
    interval: u32,
    yield_interval: u32,
}

impl WatchdogManager {
    /// Create a new WatchdogManager with specified intervals
    pub const fn new(watchdog_interval: u32, yield_interval: u32) -> Self {
        Self {
            counter: 0,
            interval: watchdog_interval,
            yield_interval,
        }
    }

    /// Update watchdog counter and reset if needed. Returns true if yield is
    /// recommended.
    pub fn update(&mut self) -> bool {
        self.counter = self.counter.saturating_add(1);

        if self.counter >= self.interval {
            log::debug!("Watchdog reset");
            self.counter = 0;
            reset_task_watchdog();
        }

        self.yield_interval > 0 && self.counter % self.yield_interval == 0
    }

    /// Reset the counter manually
    #[allow(dead_code)]
    pub fn reset(&mut self) {
        self.counter = 0;
    }
}
