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
use embedded_io_async::Read;
use embedded_io_async::Write;
use esp_hal::clock::CpuClock;
use esp_hal::spi::master::Config;
use esp_hal::timer::timg::TimerGroup;
use esp_hal::{gpio, time};
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

    defmt::info!("wifi initialized!");

    let _spawned = spawner.spawn(simulate_cpu_traffic());

    // DMA
    let dma_channel = peripherals.DMA_CH0;
    let (rx_buffer, rx_descriptors, tx_buffer, tx_descriptors) = esp_hal::dma_buffers!(10);

    let dma_rx_buf = esp_hal::dma::DmaRxBuf::new(rx_descriptors, rx_buffer).unwrap();

    let dma_tx_buf = esp_hal::dma::DmaTxBuf::new(tx_descriptors, tx_buffer).unwrap();

    defmt::info!("DMA initialized!");

    // SPI
    let mut spi_bus = esp_hal::spi::master::Spi::new(
        peripherals.SPI2,
        Config::default().with_frequency(time::Rate::from_khz(250)),
    )
    .unwrap()
    .with_sck(peripherals.GPIO4)
    .with_miso(peripherals.GPIO5)
    .with_mosi(peripherals.GPIO6)
    .with_dma(dma_channel)
    .with_buffers(dma_rx_buf, dma_tx_buf)
    .into_async();

    defmt::info!("spi bus initialized");

    let mut cs = gpio::Output::new(
        peripherals.GPIO7,
        gpio::Level::High,
        gpio::OutputConfig::default().with_pull(gpio::Pull::Up),
    );

    // Sd cards need to be clocked with a at least 74 cycles on their spi clock without the cs enabled,
    // sd_init is a helper function that does this for us.
    loop {
        match sdspi::sd_init(&mut spi_bus, &mut cs).await {
            Ok(_) => break,
            Err(e) => {
                defmt::info!("Sd init error: {:?}", e);
                embassy_time::Timer::after_millis(10).await;
            }
        }
    }

    defmt::info!("wrote buffer to spi");
    cs.set_low();

    let spid =
        embedded_hal_bus::spi::ExclusiveDevice::new(spi_bus, cs, embassy_time::Delay).unwrap();
    let mut sd = sdspi::SdSpi::<_, _, aligned::A1>::new(spid, embassy_time::Delay);

    loop {
        // Initialize the card
        match sd.init().await {
            Ok(_) => {
                // Increase the speed up to the SD max of 25mhz
                let _ = sd
                    .spi()
                    .bus_mut()
                    .apply_config(&Config::default().with_frequency(time::Rate::from_mhz(25)));
                defmt::info!("Initialization complete!");

                break;
            }
            Err(e) => defmt::info!("Failed to init card {:?}, retrying...", e),
        }
        embassy_time::Delay.delay_ns(5000u32).await;
    }

    let mut inner = block_device_adapters::BufStream::<_, 512>::new(sd);

    // 2. Read Sector 0 (The Master Boot Record)
    let mut mbr = [0u8; 512];
    inner.read_exact(&mut mbr).await.unwrap();

    // 3. Verify it's actually an SD card with an MBR
    if mbr[510] != 0x55 || mbr[511] != 0xAA {
        // If these are 0x00, your Async SPI driver is reading nothing (Hardware issue).
        // If they are random but not 0x55AA, the card is not formatted correctly.
        panic!("Invalid MBR Signature! Hardware issue or Bad Format.");
    }

    // 4. Parse Partition Table (Entry 1 is at offset 446)
    // The "LBA Start" (Logical Block Address) is a 4-byte u32 at offset 454 (446 + 8)
    let partition_start_sector = u32::from_le_bytes(mbr[454..458].try_into().unwrap());
    let partition_size_sectors = u32::from_le_bytes(mbr[458..462].try_into().unwrap());

    defmt::info!("Found Partition 1 at Sector: {}", partition_start_sector);

    // 5. Create a "Slice" of the stream that starts at the partition
    // This makes the filesystem think Byte 0 is actually Byte (partition_start * 512)
    let partition_start_byte = (partition_start_sector as u64) * 512;
    let partition_size_byte = (partition_size_sectors as u64) * 512;

    let partition_stream =
        block_device_adapters::StreamSlice::new(inner, partition_start_byte, partition_size_byte)
            .await
            .unwrap();

    let fs = embedded_fatfs::FileSystem::new(partition_stream, embedded_fatfs::FsOptions::new())
        .await
        .unwrap();
    {
        let mut f = fs.root_dir().create_file("test.log").await.unwrap();
        defmt::info!("Writing to file...");

        let mut avg = 0;

        let mut buf = [65_u8; 1024];
        for i in 1..=10 {
            core::hint::black_box(&mut buf);
            let now = time::Instant::now();
            for _ in 0..1024 {
                f.write_all(&buf).await.unwrap();
            }
            f.flush().await.unwrap();
            let diff = now.elapsed().as_millis() as i64;
            avg += (diff - avg) / i;
            defmt::info!("Writing a MB took {}", diff);
        }
        defmt::info!("The average time was: {} ms", avg);
    }

    loop {}

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
