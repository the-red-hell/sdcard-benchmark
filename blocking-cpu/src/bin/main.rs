#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]

use embassy_executor::Spawner;
use embassy_time::Timer;
use embedded_hal_bus::spi::ExclusiveDevice;
use embedded_sdmmc::sdcard::AcquireOpts;
use embedded_sdmmc::{TimeSource, Timestamp};
use esp_hal::clock::CpuClock;
use esp_hal::delay::Delay;
use esp_hal::gpio;
use esp_hal::spi::master::Spi;
use esp_hal::timer::timg::TimerGroup;
use esp_hal::{spi, time};
use esp_println::println;
use panic_rtt_target as _;

extern crate alloc;

// This creates a default app-descriptor required by the esp-idf bootloader.
// For more information see: <https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/system/app_image_format.html#application-description>
esp_bootloader_esp_idf::esp_app_desc!();

#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    // generator version: 1.0.1

    rtt_target::rtt_init_defmt!();

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 66320);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let sw_interrupt =
        esp_hal::interrupt::software::SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, sw_interrupt.software_interrupt0);

    defmt::info!("Embassy initialized!");
    println!("Embassy initialized!");

    let radio_init = esp_radio::init().expect("Failed to initialize Wi-Fi/BLE controller");
    let (mut _wifi_controller, _interfaces) =
        esp_radio::wifi::new(&radio_init, peripherals.WIFI, Default::default())
            .expect("Failed to initialize Wi-Fi controller");

    let _spawned = spawner.spawn(simulate_cpu_traffic());

    // --- SPI ---

    // // WOKWI
    // let spi_bus = Spi::new(
    //     peripherals.SPI2,
    //     spi::master::Config::default().with_frequency(time::Rate::from_khz(250)),
    // )
    // .unwrap()
    // .with_sck(peripherals.GPIO6)
    // .with_miso(peripherals.GPIO2)
    // .with_mosi(peripherals.GPIO7);

    let spi_bus = Spi::new(
        peripherals.SPI2,
        spi::master::Config::default().with_frequency(time::Rate::from_khz(100)),
    )
    .unwrap()
    .with_sck(peripherals.GPIO4)
    .with_miso(peripherals.GPIO5)
    .with_mosi(peripherals.GPIO6);

    let cs = gpio::Output::new(
        // // WOKWI
        // peripherals.GPIO10,
        peripherals.GPIO7,
        esp_hal::gpio::Level::High,
        gpio::OutputConfig::default(),
    );

    let delay = Delay::new();

    let spi_device = ExclusiveDevice::new(spi_bus, cs, delay).unwrap();

    // --- SD Card ---
    let sdcard = embedded_sdmmc::SdCard::new_with_options(
        spi_device,
        delay,
        AcquireOpts {
            use_crc: false,
            acquire_retries: 50,
        },
    );
    // println!("Card size is {:?} MB", sdcard.num_bytes());
    let num_bytes = loop {
        match sdcard.num_bytes() {
            Ok(num_bytes) => break num_bytes,
            Err(e) => {
                defmt::warn!("failed to initialize card with error {:?}", e);
                delay.delay_millis(500);
            }
        }
    };
    defmt::info!("Card size is {:?} MB", num_bytes);

    sdcard.spi(|spi| {
        spi.bus_mut()
            .apply_config(&spi::master::Config::default().with_frequency(time::Rate::from_mhz(25)))
            .unwrap()
    });
    let volume_mgr = embedded_sdmmc::VolumeManager::new(sdcard, Clock);
    defmt::info!("has opened handles: {}", volume_mgr.has_open_handles());
    let volume0 = loop {
        match volume_mgr.open_volume(embedded_sdmmc::VolumeIdx(0)) {
            Ok(volume0) => break volume0,
            Err(e) => {
                defmt::warn!("failed to open volume 0 with error {:?}", e);
                delay.delay_millis(1000);
            }
        }
    };
    println!("Volume 0: {:?}", volume0);
    defmt::info!("Volume 0: {:?}", volume0);
    let root_dir = volume0.open_root_dir().unwrap();
    println!("{:?}", root_dir.iterate_dir(|dir| println!("{:?}", dir)));
    defmt::info!("{:?}", root_dir.iterate_dir(|dir| println!("{:?}", dir)));
    let my_file =
        root_dir.open_file_in_dir("TEST.TXT", embedded_sdmmc::Mode::ReadWriteCreateOrTruncate);
    println!("{:?}", my_file);
    defmt::info!("{:?}", my_file);
    let my_file = my_file.unwrap();

    let mut avg = 0;

    let mut buf = [65_u8; 1024];
    for i in 1..=10 {
        core::hint::black_box(&mut buf);
        let now = time::Instant::now();
        for _ in 0..1024 {
            my_file.write(&buf).unwrap();
            embassy_futures::yield_now().await;
        }
        my_file.flush().unwrap();
        let diff = now.elapsed().as_millis() as i64;
        avg += (diff - avg) / i;
        println!("Writing a MB took {} ms", diff);
        defmt::info!("Writing a MB took {} ms", diff);
    }

    my_file.close().unwrap();
    root_dir.close().unwrap();
    volume0.close().unwrap();

    defmt::info!("The average time was: {} ms", avg);

    loop {}

    // for inspiration have a look at the examples at https://github.com/esp-rs/esp-hal/tree/esp-hal-v1.0.0/examples/src/bin
}

struct Clock;

impl TimeSource for Clock {
    fn get_timestamp(&self) -> embedded_sdmmc::Timestamp {
        Timestamp {
            year_since_1970: 0,
            zero_indexed_month: 0,
            zero_indexed_day: 0,
            hours: 0,
            minutes: 0,
            seconds: 0,
        }
    }
}

#[embassy_executor::task]
async fn simulate_cpu_traffic() {
    let mut buf = [0u8; 2048];
    loop {
        let mut buf1 = [0u8; 2048];
        core::hint::black_box(&mut buf1);
        buf.clone_from_slice(&buf1);
        Timer::after_micros(5).await;
    }
}
