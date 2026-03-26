use std::sync::{Arc, Mutex};

#[cfg(feature = "dotstar")]
use apa102_spi::Apa102;
use embedded_hal::spi::SpiBus;
use esp_idf_svc::hal::interrupt;
use log::{info, warn};
use smart_leds::hsv::{hsv2rgb, Hsv};
use smart_leds::{gamma, SmartLedsWrite, RGB8};
#[cfg(feature = "neopixel")]
use ws2812_spi::Ws2812;

use crate::{global, net, nvs};

pub const LED_NUM: usize = 25;

#[cfg(feature = "dotstar")]
pub struct Sleds<SPI> {
    sleds: Arc<Mutex<Apa102<SPI>>>,
}

#[cfg(feature = "neopixel")]
pub struct Sleds<SPI> {
    sleds: Arc<Mutex<Ws2812<SPI>>>,
}

impl<SPI: SpiBus> Sleds<SPI> {
    pub fn new(spi_bus: SPI) -> Self {
        #[cfg(feature = "dotstar")]
        let sleds = Apa102::new(spi_bus);

        #[cfg(feature = "neopixel")]
        let sleds = Ws2812::new(spi_bus);

        Self {
            sleds: Arc::new(Mutex::new(sleds)),
        }
    }

    pub async fn welcome(&mut self) {
        let mut hue: u16 = 0;
        for i in 0..LED_NUM {
            let mut data = [RGB8::default(); LED_NUM];
            let color = hsv2rgb(Hsv {
                hue: hue as u8,
                sat: 255,
                val: 128,
            });
            data[i] = color;
            hue = (hue + 256 / LED_NUM as u16) % 256;

            if let Ok(mut sleds) = self.sleds.lock() {
                let _ = sleds.write(gamma(data.iter().cloned()));
            }
            crate::timer::sleep_millis(50).await;
        }

        let mut data = [RGB8::default(); LED_NUM];
        for item in data.iter_mut() {
            let color = hsv2rgb(Hsv {
                hue: hue as u8,
                sat: 255,
                val: 255,
            });
            *item = color;
            hue = (hue + 256 / LED_NUM as u16) % 256;
        }

        if let Ok(mut sleds) = self.sleds.lock() {
            let _ = sleds.write(gamma(data.iter().cloned()));
        }
        crate::timer::sleep_millis(1000).await;

        // load default hsv
        if let Ok((hue, sat, val)) = nvs::get_hsv() {
            info!("Loaded HSV: hue={}, sat={}, val={}", hue, sat, val);
            if let Ok(mut h) = global::LED_HUE.lock() {
                *h = hue;
            }
            if let Ok(mut s) = global::LED_SAT.lock() {
                *s = sat;
            }
            if let Ok(mut v) = global::LED_VAL.lock() {
                *v = val;
            }
        }

        self.turn_on_all().await;
    }

    pub async fn show_time(&mut self, h: u8, m: u8) {
        let mut h = h % 24;
        let mut m10 = m / 10;
        let mut m1 = m % 10;

        // Round minutes to nearest 5
        match m1 {
            1..=2 => m1 = 0,
            3..=7 => m1 = 5,
            8..=9 => {
                m1 = 0;
                m10 += 1;
                if m10 == 6 {
                    m10 = 0;
                    h = (h + 1) % 24;
                }
            }
            _ => (),
        }

        let mut active_mask = 0u32;

        if (h == 0 || h == 12) && m10 == 0 && m1 == 0 {
            if h == 0 {
                // 자정 (Indices 15, 16)
                active_mask |= (1 << 15) | (1 << 16);
            } else {
                // 정오 (Indices 16, 21)
                active_mask |= (1 << 16) | (1 << 21);
            }
        } else {
            let h12 = if h > 12 {
                h - 12
            } else if h == 0 {
                12
            } else {
                h
            };

            // 시 (Hour)
            match h12 {
                12 => active_mask |= (1 << 0) | (1 << 5) | (1 << 14), // 열두시
                1 => active_mask |= (1 << 1) | (1 << 14),             // 한시
                2 => active_mask |= (1 << 5) | (1 << 14),             // 두시
                3 => active_mask |= (1 << 3) | (1 << 14),             // 세시
                4 => active_mask |= (1 << 4) | (1 << 14),             // 네시
                5 => active_mask |= (1 << 2) | (1 << 7) | (1 << 14),  // 다섯시
                6 => active_mask |= (1 << 6) | (1 << 7) | (1 << 14),  // 여섯시
                7 => active_mask |= (1 << 8) | (1 << 9) | (1 << 14),  // 일곱시
                8 => active_mask |= (1 << 10) | (1 << 11) | (1 << 14), // 여덟시
                9 => active_mask |= (1 << 12) | (1 << 13) | (1 << 14), // 아홉시
                10 => active_mask |= (1 << 0) | (1 << 14),            // 열시
                11 => active_mask |= (1 << 0) | (1 << 1) | (1 << 14), // 열한시
                _ => (),
            }

            // 분 (Minute)
            if m10 > 0 || m1 > 0 {
                match m10 {
                    1 => active_mask |= 1 << 22,               // 십
                    2 => active_mask |= (1 << 17) | (1 << 19), // 이십
                    3 => active_mask |= (1 << 18) | (1 << 19), // 삼십
                    4 => active_mask |= (1 << 20) | (1 << 22), // 사십
                    5 => active_mask |= (1 << 21) | (1 << 22), // 오십
                    _ => (),
                }
                if m1 == 5 {
                    active_mask |= (1 << 23) | (1 << 24); // 오분
                } else if m10 > 0 || m1 > 0 {
                    active_mask |= 1 << 24; // 분
                }
            }
        }

        self.show_leds_mask(active_mask).await;
    }

    async fn show_leds_mask(&mut self, mask: u32) {
        if let Ok(cmd) = net::get_net_cmd() {
            if !cmd.is_empty() {
                warn!("Busy with net cmd: {}. Skipping show_leds()", cmd);
                return;
            }
        }

        let hsv = Hsv {
            hue: *global::LED_HUE.lock().unwrap(),
            sat: *global::LED_SAT.lock().unwrap(),
            val: *global::LED_VAL.lock().unwrap(),
        };

        let rgb = hsv2rgb(hsv);
        let mut data = [RGB8::default(); LED_NUM];

        for i in 0..LED_NUM {
            if (mask & (1 << i)) != 0 {
                data[remap(i as u8) as usize] = rgb;
            }
        }

        let gamma_data = gamma(data.iter().cloned());

        if let Ok(mut sleds) = self.sleds.lock() {
            interrupt::free(|| {
                let _ = sleds.write(gamma_data);
            });
        }

        global::yield_to_other_tasks().await;
    }

    pub async fn turn_on_all(&mut self) {
        self.show_leds_mask((1 << LED_NUM) - 1).await;
    }
}

#[inline(always)]
fn remap(index: u8) -> u8 {
    #[cfg(feature = "tr_to_left")]
    const MAPPING: [u8; 25] = [
        4, 3, 2, 1, 0, 5, 6, 7, 8, 9, 14, 13, 12, 11, 10, 15, 16, 17, 18, 19, 24, 23, 22, 21, 20,
    ];
    #[cfg(feature = "bl_to_top")]
    const MAPPING: [u8; 25] = [
        4, 5, 14, 15, 24, 3, 6, 13, 16, 23, 2, 7, 12, 17, 22, 1, 8, 11, 18, 21, 0, 9, 10, 19, 20,
    ];
    MAPPING[index as usize]
}
