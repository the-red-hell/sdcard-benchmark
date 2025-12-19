#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]

use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use embedded_hal_bus::spi::ExclusiveDevice;
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

    let radio_init = esp_radio::init().expect("Failed to initialize Wi-Fi/BLE controller");
    let (mut _wifi_controller, _interfaces) =
        esp_radio::wifi::new(&radio_init, peripherals.WIFI, Default::default())
            .expect("Failed to initialize Wi-Fi controller");

    // TODO: Spawn some tasks
    let _ = spawner;

    // --- SPI ---
    let spi_bus = Spi::new(
        peripherals.SPI2,
        spi::master::Config::default().with_frequency(time::Rate::from_khz(250)),
    )
    .unwrap()
    .with_sck(peripherals.GPIO6)
    .with_miso(peripherals.GPIO2)
    .with_mosi(peripherals.GPIO7);
    // .with_dma(dma_channel)
    // .with_buffers(dma_rx_buf, dma_tx_buf);

    let cs = gpio::Output::new(
        peripherals.GPIO10,
        esp_hal::gpio::Level::High,
        gpio::OutputConfig::default(),
    );

    let delay = Delay::new();

    let spi_device = ExclusiveDevice::new(spi_bus, cs, delay).unwrap();

    // --- SD Card ---
    let sdcard = embedded_sdmmc::SdCard::new(spi_device, delay);
    println!("Card size is {} MB", sdcard.num_bytes().unwrap() / 1024);

    sdcard.spi(|spi| {
        spi.bus_mut()
            .apply_config(&spi::master::Config::default().with_frequency(time::Rate::from_mhz(80)))
            .unwrap()
    });
    let volume_mgr = embedded_sdmmc::VolumeManager::new(sdcard, Clock);
    let volume0 = volume_mgr
        .open_volume(embedded_sdmmc::VolumeIdx(0))
        .unwrap();
    println!("Volume 0: {:?}", volume0);
    let root_dir = volume0.open_root_dir().unwrap();
    println!("{:?}", root_dir.iterate_dir(|dir| println!("{:?}", dir)));
    let my_file =
        root_dir.open_file_in_dir("TEST.TXT", embedded_sdmmc::Mode::ReadWriteCreateOrAppend);
    println!("{:?}", my_file);
    let my_file = my_file.unwrap();

    let mut buf = [65_u8; 1024];
    loop {
        core::hint::black_box(&mut buf);
        let now = time::Instant::now();
        for _ in 0..1024 {
            my_file.write(&buf).unwrap();
        }
        let diff = now.elapsed();
        println!("Writing a MB took {}", diff.as_millis());
    }

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
