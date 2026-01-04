I need a fast SD-card implementation for a side-project and I found that there are quite a few out there, but most of them (mainly embedded-sdmmc) is optimised for readability/simplicity instead of performance. So I'll write a few small benchmarks to test how they compare
The main issue might even be FAT formatting. If it would just be the SPI device I'm writing to, I might as well also write my own library. I won't do FAT (16/32) though.

The optimal flow would be asynchronously and just doing DMA -> SD card, without any copying. That way I have basically 100% of the CPU available. This is, as it turns out, not as easy as expected.

The measurements do not really show real-world scenarios. They just write to the sd-card and an external task (copying 2 KiB every 5 us) to simulate asynchrony.
I always write one mebibyte in buffers of 1 kibibytes (so essentially writing 1024 blocks of 1024 bytes).

## some infos
- device in use: ESP32C3
- SD-card reader: no fucking clue, some thing I found on AliExpress
- SPI frequency: 25Mhz (this is pretty much the maximum)

## results

### blocking-cpu
This one uses [embedded-sdmmc](https://docs.rs/embedded-sdmmc/latest/embedded_sdmmc) under the hood, blocks and uses the CPU to transfer data.

#### Wokwi

It was something along the lines of 20 seconds. As this absolutely does not correspond to the real-world-scenario, I will not consider benchmarking with this any further.

#### irl
average out of 20 writes:
**1686 ms**

### async-dma
This one uses [embedded-fatfs](https://github.com/MabezDev/embedded-fatfs) under the hood, has asynchronous write-calls and uses the DMA buffer to write from (yes, the data still has to be copied on there).

#### Wokwi
I sadly cannot simulate it on Wokwi, since there seems to be an issue using dma and async, not entirely sure why since they say it's fully implemented (ofc it can also be an issue with the library itself, but I haven't succeeded in writing to the SPI device at all using async (and the transfer method, read and write works)). 

#### irl
average out of 20 writes:
**7089 ms**

---

## Conclusion
Blocking is definitely faster. This could have many different reasons in which I won't dig into further.
One consideration is definitely the overhead of asynchronous writes with many many interrupts.

It is also important to note that async might be faster if we have more stuff running in the background, which can't be said with 100% certainty though (as I already tried to simulate background traffic).