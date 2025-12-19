#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]

use embassy_executor::Spawner;
use embassy_time::Timer;
use embedded_hal_async::delay::DelayNs as _;
use embedded_io_async::Write;
use esp_hal::clock::CpuClock;
use esp_hal::spi::master::Config;
use esp_hal::timer::timg::TimerGroup;
use esp_hal::{gpio, time};
use esp_println::println;
use panic_rtt_target as _;
use rtt_target::rprintln;

extern crate alloc;

// This creates a default app-descriptor required by the esp-idf bootloader.
// For more information see: <https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/system/app_image_format.html#application-description>
esp_bootloader_esp_idf::esp_app_desc!();

#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    // generator version: 1.0.1

    rtt_target::rtt_init_print!();

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 66320);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let sw_interrupt =
        esp_hal::interrupt::software::SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, sw_interrupt.software_interrupt0);

    rprintln!("Embassy initialized!");

    let radio_init = esp_radio::init().expect("Failed to initialize Wi-Fi/BLE controller");
    let (mut _wifi_controller, _interfaces) =
        esp_radio::wifi::new(&radio_init, peripherals.WIFI, Default::default())
            .expect("Failed to initialize Wi-Fi controller");

    // TODO: Spawn some tasks
    let _spawned = spawner.spawn(simulate_cpu_traffic());

    // DMA
    let dma_channel = peripherals.DMA_CH0;
    let (rx_buffer, rx_descriptors, tx_buffer, tx_descriptors) = esp_hal::dma_buffers!(16000);

    let dma_rx_buf = esp_hal::dma::DmaRxBuf::new(rx_descriptors, rx_buffer).unwrap();

    let dma_tx_buf = esp_hal::dma::DmaTxBuf::new(tx_descriptors, tx_buffer).unwrap();

    // SPI
    let mut spi_bus = esp_hal::spi::master::Spi::new(
        peripherals.SPI2,
        Config::default().with_frequency(time::Rate::from_khz(250)), //  max: 80MHz
    )
    .unwrap()
    .with_sck(peripherals.GPIO6)
    .with_miso(peripherals.GPIO2)
    .with_mosi(peripherals.GPIO7)
    .with_dma(dma_channel)
    .with_buffers(dma_rx_buf, dma_tx_buf)
    .into_async();

    println!("spi bus initialized");

    let mut cs = gpio::Output::new(
        peripherals.GPIO10,
        gpio::Level::High,
        gpio::OutputConfig::default(),
    );

    // Sd cards need to be clocked with a at least 74 cycles on their spi clock without the cs enabled,
    // sd_init is a helper function that does this for us.
    loop {
        match sdspi::sd_init(&mut spi_bus, &mut cs).await {
            Ok(_) => break,
            Err(e) => {
                println!("Sd init error: {:?}", e);
                embassy_time::Timer::after_millis(10).await;
            }
        }
    }

    let spid =
        embedded_hal_bus::spi::ExclusiveDevice::new(spi_bus, cs, embassy_time::Delay).unwrap();
    let mut sd = sdspi::SdSpi::<_, _, aligned::A1>::new(spid, embassy_time::Delay);

    loop {
        // Initialize the card
        if sd.init().await.is_ok() {
            // Increase the speed up to the SD max of 25mhz
            let _ = sd
                .spi()
                .bus_mut()
                .apply_config(&Config::default().with_frequency(time::Rate::from_mhz(80)));
            println!("Initialization complete!");

            break;
        }
        println!("Failed to init card, retrying...");
        embassy_time::Delay.delay_ns(5000u32).await;
    }

    let inner = block_device_adapters::BufStream::<_, 512>::new(sd);

    let fs = embedded_fatfs::FileSystem::new(inner, embedded_fatfs::FsOptions::new())
        .await
        .unwrap();
    {
        let mut f = fs.root_dir().create_file("test.log").await.unwrap();
        println!("Writing to file...");

        let mut buf = [65_u8; 1024];
        loop {
            core::hint::black_box(&mut buf);
            let now = time::Instant::now();
            for _ in 0..1024 {
                f.write_all(&buf).await.unwrap();
            }
            f.flush().await.unwrap();
            let diff = now.elapsed();
            println!("Writing a MB took {}", diff.as_millis());
        }
    }

    // for inspiration have a look at the examples at https://github.com/esp-rs/esp-hal/tree/esp-hal-v1.0.0/examples/src/bin
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
