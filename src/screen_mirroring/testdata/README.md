`aac-eld-480.bin` is synthetic test audio, not a device recording. It contains
12 raw AAC-ELD access units, each preceded by a little-endian u32 byte count.
It was generated from a 440 Hz stereo sine wave (amplitude 2000 in signed
16-bit PCM) using FDK AAC: AOT 39, 48,000 Hz, MODE_2, 128,000 bit/s,
AACENC_GRANULE_LENGTH=480, and raw transport (AACENC_TRANSMUX=0).
The resulting AudioSpecificConfig is `f8 e6 50 00`.

The regression test decodes these frames with the production GStreamer decoder
configuration and checks that each decoded frame contains 480 stereo samples.
The encoder is only needed to regenerate the fixture; it is not a runtime or
CI dependency.
