# Recording from an input that OBS and Zoom also read

Research for [#234](https://github.com/incrhst/lampstand/issues/234), part of the
[Recording the sermon map](https://github.com/incrhst/lampstand/issues/233). It decides
T117's input handling and the channel choice in [decision 0012](https://github.com/incrhst/lampstand/blob/claude/nostalgic-swirles-002ca3/docs/decisions/0012-recording-the-sermon.md).
Checked against the cpal 0.16.0 source (`~/.cargo/registry/src/*/cpal-0.16.0`), the version
`lampstand-media` already uses. Researched 2026-10-25.

## Short answer

Yes, on macOS and Windows. cpal opens every input in shared mode, and so do OBS and Zoom.
Reading an input does not change it for anyone else, with one catch: on macOS, cpal changes
the device's sample rate for every app if Lampstand asks for a rate the device is not running
at. So Lampstand must open the input at the rate and channel count it is already running at,
take every channel, and pick channel 1 (or a pair) itself.

## macOS (Core Audio)

**Confirmed from source and docs**

- cpal opens an input through an AUHAL (`kAudioUnitSubType_HALOutput`) audio unit with input
  enabled (`audio_unit_from_device` in `src/host/coreaudio/macos/mod.rs`). It never asks for
  hog mode, Core Audio's only exclusive lock. Core Audio mixes and shares devices by default.
  Apple's engineers advise hog mode only for data that cannot be mixed, such as AC-3
  ([Apple forums](https://developer.apple.com/forums/thread/70274)).
- OBS opens its macOS input the same way, through `kAudioUnitSubType_HALOutput`
  ([obs-studio `plugins/mac-capture/mac-audio.c`](https://github.com/obsproject/obs-studio/blob/master/plugins/mac-capture/mac-audio.c)).
- **The catch.** `build_input_stream_raw` calls `set_sample_rate` first. If the requested
  rate is not the device's current nominal rate, it sets `kAudioDevicePropertyNominalSampleRate`
  on the device. That is a device-wide setting: OBS and Zoom see the device change rate under
  them, which can click, drop out or make them reopen the device. Asking for the current rate
  changes nothing.
- The AUHAL does no sample-rate conversion on input: "the device sample rate must match the
  desired sample rate" ([Apple TN2091](https://developer.apple.com/library/archive/technotes/tn2091/_index.html)).
  So if another app changes the rate mid-recording, Lampstand's stream is no longer at the rate
  its WAV header says. cpal 0.16 does not report this.
- `supported_input_configs` reports one entry per rate range the device lists
  (`kAudioDevicePropertyAvailableNominalSampleRates`), always as `F32`, with the channel count
  summed over all the device's input streams. `default_input_config` reads the device's current
  stream format (normally `F32`, at the current rate).
- Channel selection: cpal has no channel map. Core Audio's `kAudioOutputUnitProperty_ChannelMap`
  can pick device channels ([TN2091](https://developer.apple.com/library/archive/technotes/tn2091/_index.html)),
  but cpal does not set it. Opening fewer channels than the device has gives the first N
  (the default mapping). To get channel 2 alone, or 3 and 4, open all channels and pick in the
  callback.
- `BufferSize::Fixed` sets `kAudioDevicePropertyBufferFrameSize`. `BufferSize::Default` leaves it.
- Class-compliant interfaces use Apple's own USB audio driver. A Scarlett 2i2 offers 44.1 to
  192 kHz at 24 bit ([Focusrite spec](https://www.sweetwater.com/sweetcare/wp-json/wp/v2/articles/1761962)).
  Larger units report fewer channels at higher rate bands
  ([Focusrite](https://support.focusrite.com/hc/en-gb/articles/28487570308114)). Apps and DAWs
  can override the rate set in Audio MIDI Setup (same source).

**Needs a real-hardware check**

- That a Scarlett and a UMC report 2 input channels at 48 kHz `F32` through cpal, and what a
  UMC404HD reports.
- That opening the device at its current rate makes no click or level change in OBS or Zoom.
- Whether Zoom or OBS ever change the device rate themselves during a service.
- Whether the input volume slider in Sound settings is live for these interfaces (it is often
  missing for interfaces with hardware gain knobs).

## Windows (WASAPI)

**Confirmed from source and docs**

- cpal always calls `IAudioClient::Initialize` with `AUDCLNT_SHAREMODE_SHARED` and
  `AUDCLNT_STREAMFLAGS_EVENTCALLBACK` (`src/host/wasapi/device.rs`). The source says so:
  "We always create voices in shared mode". It never takes exclusive mode.
- OBS captures inputs in shared mode too: `client->Initialize(AUDCLNT_SHAREMODE_SHARED, ...)`
  ([obs-studio `plugins/win-wasapi/win-wasapi.cpp`](https://github.com/obsproject/obs-studio/blob/master/plugins/win-wasapi/win-wasapi.cpp)).
- Shared mode is the way several clients use one endpoint; the audio engine hands each one its
  own copy. `AUDCLNT_E_DEVICE_IN_USE` happens only when some app holds the device in
  exclusive mode ([IAudioClient::Initialize](https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nf-audioclient-iaudioclient-initialize),
  [Exclusive-mode streams](https://learn.microsoft.com/en-us/windows/win32/coreaudio/exclusive-mode-streams)).
  If a DAW or similar takes the interface exclusively, Lampstand cannot open it and must say why.
- A shared-mode stream must use the engine's mix format unless the client passes
  `AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM` ([Device formats](https://learn.microsoft.com/en-us/windows/win32/coreaudio/device-formats)).
  cpal does not pass it. In practice cpal accepts only the mix format: its own comment says any
  other channel count fails, and other rates fail `IsFormatSupported`.
- So on Windows: `default_input_config` returns the mix format (normally `F32`, the channel count
  and rate set under Sound > Recording > device > Advanced). Lampstand gets that or nothing. It
  cannot change the rate, so it cannot disturb OBS or Zoom.
- Scarlett on Windows uses Focusrite's driver. The rate set in Focusrite Device Settings "will be
  the only option that appears" in the Windows Sound panel
  ([Focusrite](https://support.focusrite.com/hc/en-gb/articles/208814065)).

**Needs a real-hardware check**

- The endpoints and channel counts a Scarlett and a UMC show on Windows (one stereo endpoint, or
  one per pair on larger units).
- Zoom's "Automatically adjust microphone volume" changes the input level. Users report it does
  so even when turned off ([Zoom community](https://community.zoom.com/meetings-2/zoom-auto-adjusts-microphone-even-thought-that-setting-is-turned-off-windows-11-32383)).
  If it moves the Windows endpoint volume, that volume is shared, so Lampstand's recording level
  moves too. Lampstand itself never changes it. To check: watch Lampstand's meter during a Zoom
  call with the setting on and off.

## Does a third reader change levels or cause glitches?

- **Levels.** Opening and reading an input changes nothing on macOS or Windows. Neither cpal path
  sets volume or gain. Levels change only if some app changes the device's own volume (Zoom's
  auto-adjust, above) or, on macOS, its rate.
- **Glitches.** Each shared client gets its own buffer, so a slow reader drops its own audio, not
  OBS's. Lampstand's callback must only copy samples into a queue; the WAV writer runs on another
  thread. Whether the 2019 laptop keeps up beside OBS is T61's measurement, not this one.

## Linux (ALSA, PipeWire)

**Confirmed from source**

- cpal 0.16 on Linux uses ALSA. It lists `default`, `pipewire`, `pulse`, `jack` and `oss` first,
  then each card as `plughw:N` (`src/host/alsa/enumerate.rs`). `plughw` is a direct hardware
  device with ALSA's conversion layer. The source notes it may resample and convert formats "under
  your feet", and is used because cpal cannot read 24-bit packed samples.
- A `hw`/`plughw` device can be opened by one process only. On a PipeWire desktop, PipeWire holds
  the card, so `plughw:N` fails with "Device or resource busy"
  ([PipeWire #3570](https://gitlab.freedesktop.org/pipewire/pipewire/-/issues/3570),
  [PipeWire #779](https://gitlab.freedesktop.org/pipewire/pipewire/-/issues/779)).
  Sharing works only through `pipewire` or `default`, which go through PipeWire's ALSA plugin.
- Through `pipewire`/`default`, Lampstand gets the *default* source, not a named interface,
  unless the user picks it in the system's sound settings. cpal 0.16 has no PipeWire host of its own.

**Needs a check**

- Linux runs in CI only. Treat it as: record from `default`, tell the user to choose the
  interface in system sound settings, and do not promise sharing on a bare-ALSA system.

## Recommendation

**T117 (input handling)**

1. Open the chosen input with `default_input_config()`: its rate, channel count and sample format
   as they are. Never ask for another rate. On macOS that would change the rate for OBS and Zoom;
   on Windows it fails.
2. Use `BufferSize::Default`.
3. Accept `F32` (the normal case on both systems) and `I16`/`I32`, and convert to 24-bit integer
   when writing the WAV.
4. Record the rate in the WAV header from the opened config. On macOS, listen for
   `kAudioDevicePropertyNominalSampleRate` changes (cpal 0.16 does not); if the rate changes,
   end the part and start part 2 at the new rate, as decision 0012 already does for a restart.
5. Turn build errors into the role bar's reasons: device in use by another app (Windows
   exclusive mode, macOS hog mode), device gone, no permission.
6. Never set input volume, gain or rate. Lampstand only reads.
7. Linux: list `default` first and say the interface is chosen in system sound settings.
   Hide `plughw:N` entries, or show their "busy" error plainly.

**Channel choice (ticket 7)**

- Always open all the device's channels, and take the chosen ones in the callback. cpal can only
  give "the first N" on macOS and "all of them" on Windows, so selecting in Lampstand is the one
  way that works the same everywhere.
- Setup > Recording offers: "Channel 1", "Channel 2" ... (mono, one per device channel) and
  "Channels 1 and 2", "3 and 4" ... (stereo pairs). Default: **Channel 1, mono**. A mixer feed
  into a 2-input interface is usually one aux send into input 1, and mono halves the file size.
- Store the choice as channel indexes, not labels. If the device later reports fewer channels,
  fall back to channel 1 and say so in Setup.

## Hardware check before release

On a Mac and a Windows laptop, with a Scarlett 2i2 and a Behringer UMC, with OBS and a Zoom call
both capturing the interface:

- cpal reports the rate, format and channel count expected above.
- Lampstand starts and stops recording five times; OBS and Zoom show no click, dropout or level change.
- Zoom's auto-adjust on and off: does Lampstand's meter move?
- macOS: change the rate in Audio MIDI Setup mid-recording; Lampstand starts part 2.
- Windows: an app holding the device exclusively gives a plain "in use" message.
