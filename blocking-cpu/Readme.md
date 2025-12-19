This example shows a blocking, CPU-controlled data transfer

Simulating it on Wokwi gives following results when writing one mebibyte in buffers of 1 kibibytes (so essentially writing 1024 blocks of 1024 bytes)
(in ms):

- 19518
- 19521
- 19522
- 19522
- 19522 

This is pretty accurately 20 seconds.