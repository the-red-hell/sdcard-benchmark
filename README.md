I need a fast SD-card implementation for a side-project and I found that there are quite a few out there, but most of them (mainly embedded-sdmmc) is optimised for readability/simplicity instead of performance. So I'll write a few small benchmarks to test how they compare
The main issue might even be FAT formatting. If it would just be the SPI device I'm writing to, I might as well also write my own library. I won't do FAT (16/32) though.

The optimal flow would be asynchronously and just doing DMA -> SD card, without any copying. That way I have basically 100% of the CPU available. This is, as it turns out, not as easy as expected.

The measurements do not really show real-world scenarios. They just write to the sd-card and an external task (doing memcpy to do sth for the cpu) to simulate asynchrony.

## some infos
- device in use: ESP32C3
- SD-card reader: no fucking clue
- SPI frequency: 80Mhz (as stated in [the doc](https://documentation.espressif.com/api/resource/doc/file/aY69Zg1p/FILE/esp32-c3_technical_reference_manual_en.pdf#section.27.3))

## results

### blocking-cpu
This one uses [embedded-sdmmc](https://docs.rs/embedded-sdmmc/latest/embedded_sdmmc) under the hood, blocks and uses the CPU to transfer data.

Simulating it on Wokwi gives following results when writing one mebibyte in buffers of 1 kibibytes (so essentially writing 1024 blocks of 1024 bytes)
(in ms):

- 19751
- 19754
- 19755
- 19755
- 19755

This is pretty much 20 seconds and the CPU is ~65-70% busy.

### async-dma
This one uses [embedded-fatfs](https://github.com/MabezDev/embedded-fatfs) under the hood, has asynchronous write-calls and uses the DMA buffer to write from (yes, the data still has to be copied on there).

I sadly cannot simulate it on Wokwi, since there seems to be an issue using dma and async, not entirely sure why since they say it's fully implemented (ofc it can also be an issue with the library itself, but I haven't suceeded in writing to the SPI device at all using async (and the transfer method, read and write works)). 
