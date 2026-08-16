use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::{self, Duration, Instant};

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

/// Cooperative task identities for stall detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub enum TaskId {
    Net = 0,
    TimeSync = 1,
    ShowTime = 2,
    Menu = 3,
    Rotary = 4,
}

const TASK_COUNT: usize = 5;
const DEFAULT_NET_STALL_MS: u32 = 90_000;

/// Per-task stall limits. Net is overridden by [`set_net_stall_limit_ms`].
const TASK_STALL_LIMIT_MS: [u32; TASK_COUNT] = [
    DEFAULT_NET_STALL_MS, // Net
    180_000,              // TimeSync sleeps 60s between iterations
    90_000,               // ShowTime (must outlast blocking WiFi/HTTP timeouts)
    90_000,               // Menu
    90_000,               // Rotary
];

static MONOTONIC_ORIGIN: Lazy<Instant> = Lazy::new(Instant::now);
static HEARTBEAT_MS: [AtomicU32; TASK_COUNT] = [
    AtomicU32::new(0),
    AtomicU32::new(0),
    AtomicU32::new(0),
    AtomicU32::new(0),
    AtomicU32::new(0),
];
static NET_STALL_LIMIT_MS: AtomicU32 = AtomicU32::new(DEFAULT_NET_STALL_MS);

fn monotonic_ms() -> u32 {
    MONOTONIC_ORIGIN.elapsed().as_millis() as u32
}

fn task_name(id: usize) -> &'static str {
    match id {
        0 => "net_loop",
        1 => "time_sync_loop",
        2 => "show_time_loop",
        3 => "menu_loop",
        4 => "rotary_encoder_loop",
        _ => "unknown",
    }
}

/// Record that `task` is still making progress.
pub fn heartbeat(task: TaskId) {
    HEARTBEAT_MS[task as usize].store(monotonic_ms(), Ordering::Relaxed);
}

/// Stretch or shrink the net_loop stall budget for the current command.
///
/// OTA should keep the default-ish budget and call [`heartbeat`] on download
/// progress instead of allowing a multi-minute silent hang.
pub fn set_net_stall_limit_ms(ms: u32) {
    NET_STALL_LIMIT_MS.store(ms, Ordering::Relaxed);
}

pub fn reset_net_stall_limit() {
    set_net_stall_limit_ms(DEFAULT_NET_STALL_MS);
}

/// Independent FreeRTOS thread: restarts the device if a task stops heartbeating.
///
/// `embassy_time::with_timeout` cannot interrupt blocking ESP-IDF calls
/// (HTTP `submit`/`read`, some WiFi ops). This watchdog still fires because it
/// does not share the async executor.
pub fn start_stall_watchdog() {
    let now = monotonic_ms();
    for hb in &HEARTBEAT_MS {
        hb.store(now, Ordering::Relaxed);
    }

    let spawn_result = std::thread::Builder::new()
        .name("stall-wdt".into())
        .stack_size(8 * 1024)
        .spawn(|| loop {
            std::thread::sleep(Duration::from_secs(5));
            let now = monotonic_ms();
            for (i, hb) in HEARTBEAT_MS.iter().enumerate() {
                let last = hb.load(Ordering::Relaxed);
                if last == 0 {
                    continue;
                }
                let stalled = now.wrapping_sub(last);
                let limit = if i == TaskId::Net as usize {
                    NET_STALL_LIMIT_MS.load(Ordering::Relaxed)
                } else {
                    TASK_STALL_LIMIT_MS[i]
                };
                if stalled > limit {
                    log::error!(
                        "Stall watchdog: {} silent for {}ms (limit {}ms), restarting",
                        task_name(i),
                        stalled,
                        limit
                    );
                    esp_idf_svc::hal::reset::restart();
                }
            }
        });

    match spawn_result {
        Ok(_) => log::info!("Stall watchdog thread started"),
        Err(e) => log::warn!("Failed to start stall watchdog: {e}"),
    }
}

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
    task: TaskId,
}

impl WatchdogManager {
    /// Create a new WatchdogManager with specified intervals
    pub const fn new(task: TaskId, watchdog_interval: u32, yield_interval: u32) -> Self {
        Self {
            counter: 0,
            interval: watchdog_interval,
            yield_interval,
            task,
        }
    }

    /// Update watchdog counter and reset if needed. Returns true if yield is
    /// recommended.
    pub fn update(&mut self) -> bool {
        heartbeat(self.task);
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
