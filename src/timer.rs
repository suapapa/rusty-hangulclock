// use std::{thread, time};

use embassy_time::{Duration, Timer};

// pub fn sleep_hours(hours: u64) {
//     thread::sleep(time::Duration::from_secs(hours * 3600));
// }

pub async fn sleep_secs(secs: u64) {
    // thread::sleep(time::Duration::from_secs(secs));
    Timer::after(Duration::from_secs(secs)).await;
}

pub async fn sleep_millis(millis: u64) {
    // thread::sleep(time::Duration::from_millis(millis));
    Timer::after(Duration::from_millis(millis)).await;
}
