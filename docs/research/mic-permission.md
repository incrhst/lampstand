# The microphone permission in a signed Tauri app (macOS and Windows)

Research for [#236](https://github.com/incrhst/lampstand/issues/236), part of the
[Recording the sermon](https://github.com/incrhst/lampstand/issues/233) map. It
decides how Setup > Recording asks for the permission and how `PermissionDenied`
(T120, T121) is detected. Date: 2026-10-25.

## Short answer

- **macOS needs both.** `NSMicrophoneUsageDescription` in Info.plist *and* the
  `com.apple.security.device.audio-input` entitlement. Under the hardened runtime,
  without the entitlement, the system terminates the app when it touches the
  microphone. It does not just stay silent.
- **The macOS prompt comes on first use.** Opening and starting a cpal input stream
  is enough to show it. Lampstand can also show it on purpose with
  `AVCaptureDevice requestAccessForMediaType:` through `objc2-av-foundation`.
- **After a refusal on macOS, cpal opens the stream and delivers zeros.** There is
  no error. The app tells by asking AVFoundation for the authorisation status.
- **Windows shows no prompt to a desktop app.** One switch, "Let desktop apps access
  your microphone", covers every desktop app. When it is off, WASAPI fails with
  `E_ACCESSDENIED` (0x80070005). cpal 0.16 passes that on as a
  `BackendSpecific` error (or, at one step, as `DeviceNotAvailable`).
- **Both systems have a Settings link.** `ms-settings:privacy-microphone` on Windows,
  `x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone` on
  macOS.

## How Lampstand is set up today

- `app/src-tauri/tauri.conf.json`: `bundle.macOS.hardenedRuntime: true`,
  `bundle.macOS.entitlements: "entitlements.plist"`, minimum macOS 11.
- `app/src-tauri/entitlements.plist`: empty `<dict/>`. Its comment says to add an
  entitlement only if a notarised build fails without it. The microphone is such a
  case.
- `app/src-tauri/Info.plist`: merged into the bundle's Info.plist by the Tauri
  bundler. It holds `NSLocalNetworkUsageDescription` and `NSBonjourServices` today.
- `.github/workflows/release.yml`: `tauri-action` signs and notarises with the
  `APPLE_*` secrets, then the disk image is notarised with `notarytool`. The
  Windows installer is not signed yet.
- cpal 0.16 is already used for output (`crates/lampstand-media`). `objc2` 0.5 is
  used on macOS for display names; `objc2` 0.6, `block2` 0.6 and
  `objc2-foundation` 0.3 are already in `Cargo.lock` through Tauri.

## macOS

### Info.plist key and entitlement: both are needed

Apple's AVFoundation guide says an app that uses microphones includes
`NSMicrophoneUsageDescription` in Info.plist, and that in macOS it must also enable
`com.apple.security.device.audio-input`. It warns that without the key and the
entitlement "the system terminates your app"
([Apple: Requesting authorization to capture and save media](https://developer.apple.com/documentation/avfoundation/requesting-authorization-to-capture-and-save-media)).

The hardened runtime guide lists "Audio Input" as a resource-access entitlement:
"record audio using the built-in microphone and access audio input using the Core
Audio APIs". It also says that the entitlement does not replace asking the person
([Apple: Configuring the hardened runtime](https://developer.apple.com/documentation/xcode/configuring-the-hardened-runtime)).
cpal's macOS host is Core Audio, so this applies to it.

A developer on the Apple forums had the usage text but no entitlement: the notarised
app was killed when it asked for the microphone. Adding the entitlement made the
prompt appear ([Apple Developer Forums 696384](https://developer.apple.com/forums/thread/696384)).
The `audio-input` key is the one for apps notarised and shipped outside the Mac App
Store ([Unity Vivox: macOS requirements](https://docs.unity.com/vivox-core/developer-guide/macos/macos-requirements)).

### How Tauri 2 adds them

- **Info.plist:** Tauri merges `src-tauri/Info.plist` with the one it generates.
  Tauri's own example is `NSMicrophoneUsageDescription`
  ([Tauri: macOS application bundle](https://v2.tauri.app/distribute/macos-application-bundle/)).
  Lampstand already has this file, so the key goes beside the Local Network keys.
- **Entitlements:** the file named in `bundle.macOS.entitlements` is applied when the
  app is signed (same page). Lampstand already points at `entitlements.plist`, so the
  key goes there:

```xml
<key>com.apple.security.device.audio-input</key>
<true/>
```

- Check a built app with `codesign -d --entitlements - Lampstand.app` and
  `plutil -p Lampstand.app/Contents/Info.plist`.
- The text can be translated later with `InfoPlist.strings` in `<lang>.lproj`
  folders (same Tauri page).

### When the prompt appears

- The system shows the alert "before your app can use a capture device for the first
  time", and remembers the answer (Apple guide above).
- With cpal, starting an input stream is the first use. A cpal user saw Terminal's
  microphone prompt the first time a cpal input demo ran
  ([cpal #329](https://github.com/RustAudio/cpal/issues/329)).
- Some cpal calls touch the input side even for output. In cpal before 0.17, getting
  the default output device could show the microphone prompt
  ([cpal #901](https://github.com/RustAudio/cpal/issues/901), fixed by
  [cpal #1070](https://github.com/RustAudio/cpal/pull/1070) in 0.17). With the
  entitlement added, this matters: a fresh Mac must not see the prompt when
  Lampstand only plays sound. A test is listed below.
- **To ask on purpose:** Apple says to check `AVCaptureDevice authorizationStatus`
  before capture, and call `requestAccess(for:completionHandler:)` when it is
  `notDetermined`, "only at a time that's appropriate for your app" (Apple guide
  above). From Rust this is `objc2-av-foundation` (0.3, built on `objc2` 0.6 and
  `block2` 0.6): `AVCaptureDevice::authorizationStatusForMediaType(AVMediaTypeAudio)`
  and `AVCaptureDevice::requestAccessForMediaType_completionHandler`. The
  `moq-audio` crate does exactly this before it opens a cpal stream, and waits for
  the answer on a channel with a timeout
  ([moq-audio permission.rs](https://docs.rs/crate/moq-audio/0.0.15/source/src/capture/permission.rs)).
- The microphone permission covers every input device, built-in or USB.

### What cpal sees after a refusal

- The stream opens and plays. The callback gets buffers of zeros. "Toggling the
  permission toggled whether non-zero samples were produced"
  ([cpal #329](https://github.com/RustAudio/cpal/issues/329)).
- `moq-audio` says the same: "cpal can't see macOS TCC, so a denied mic just yields
  silence (or no callbacks at all)" (permission.rs above).
- **So the app tells by asking AVFoundation, not cpal.** The status is one of
  `NotDetermined`, `Restricted` (MDM or parental controls), `Denied` or
  `Authorized`. Restricted cannot be changed by the person.
- After the first answer, the status changes only when the person changes it in
  System Settings, so Setup should read it again when it gets focus.

### Resetting it for testing

- `tccutil reset Microphone org.lampstand.app` forgets the answer for Lampstand. The
  next first use prompts again
  ([Apple: Resetting access to protected resources](https://developer.apple.com/documentation/xcode/resetting-access-to-protected-resources-in-macos)).
  `tccutil reset Microphone` with no bundle ID resets every app.
- The permission is tied to the signed app. A build run from a terminal (`tauri dev`
  or `cargo run`) is usually attributed to the terminal app, so test the prompt with
  a signed, bundled build. Reset the terminal with
  `tccutil reset Microphone com.apple.Terminal` (as cpal #901 does).

## Windows

### The setting

- Settings > Privacy & security > Microphone has "Microphone access" (the whole
  device), "Let apps access your microphone", and "Let desktop apps access your
  microphone". Desktop apps "cannot be individually toggled"; the one desktop-apps
  switch covers them all, and only for apps "that use supported methods"
  ([Microsoft: Windows camera, microphone, and privacy](https://support.microsoft.com/en-us/windows/windows-camera-microphone-and-privacy-a83257bc-e990-d54a-d212-b5e41beba857),
  [Microsoft: Turn on app permissions for your microphone](https://support.microsoft.com/en-us/windows/turn-on-app-permissions-for-your-microphone-94991183-f69d-b4cf-4679-c98ca45f577a)).
- Lampstand is a desktop (unpackaged NSIS) app, so there is no prompt and no per-app
  answer to ask for. Windows 11 Insider builds are adding per-app switches for
  desktop apps
  ([Windows Central](https://www.windowscentral.com/microsoft/windows-11/windows-11-will-soon-let-you-block-camera-and-mic-access-for-any-individual-desktop-app));
  the same link and the same error cover that case.
- The answers are kept under
  `HKCU\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone`
  (`Value` = `Allow` or `Deny`), with desktop apps under its `NonPackaged` subkey,
  and the device-wide switch under the same path in `HKLM`. This is not documented by
  Microsoft ([Eleven Forum thread](https://www.elevenforum.com/t/windows-11-microphone-access-settings-disable-globally-enable-by-app.37006/)),
  so Lampstand should not rely on it.

### What WASAPI and cpal see when access is off

- WASAPI fails with `0x80070005` (`E_ACCESSDENIED`). OBS logs "Failed to get
  initialize audio client: 80070005" for a microphone source, fixed by turning on
  microphone access ([OBS forum](https://obsproject.com/forum/threads/failed-to-get-initialize-audio-client-80070005-log-included.108338)).
  Other apps see it when they activate `IAudioClient`
  ([Meetily permission handling](https://instagit.com/Zackriya-Solutions/meetily/audio-permission-handling-cross-platform.md)).
- cpal 0.16 (`src/host/wasapi/device.rs`, `build_input_stream_raw_inner`): an error
  from `Activate` or `Initialize` becomes `BuildStreamError::BackendSpecific` with the
  Windows message, which includes `0x80070005` and "Access is denied". An error from
  `IsFormatSupported` becomes `BuildStreamError::DeviceNotAvailable`, which looks
  like an unplugged device. Which step fails first was not tested here.
- So on Windows the failure is an error at open, not silence. That makes it easy to
  detect, but the error has to be read for the code.

### Pointing the person to it

- `ms-settings:privacy-microphone` opens the Microphone privacy page. Microsoft
  recommends exactly this link when an app cannot use the microphone
  ([Microsoft: Launch Windows Settings](https://learn.microsoft.com/en-us/windows/apps/develop/launch/launch-settings-app)).
- `obs_control::open_url` already opens a URL on Windows with
  `rundll32 url.dll,FileProtocolHandler`, which handles `ms-settings:` too.

## Recommendation

### How Setup > Recording asks

1. Setup > Recording shows the input list without opening any input. Listing names
   with cpal does not need the permission. (Check on a reset Mac that it does not
   prompt; see the test below.)
2. When the person chooses an input, and only then:
   - **macOS:** read the AVFoundation status. `NotDetermined`: call
     `requestAccessForMediaType` and wait for the answer, with Setup saying "Allow
     Lampstand to use the microphone so it can record the sermon." `Denied`: show
     the reason and an **Open System Settings** button
     (`x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone`).
     `Restricted`: say the computer's administrator has blocked it, with no button.
   - **Windows:** open the chosen input briefly (start, wait for the first buffer,
     stop). `E_ACCESSDENIED` means `PermissionDenied`: show the reason and an
     **Open Settings** button (`ms-settings:privacy-microphone`), and say to turn on
     "Let desktop apps access your microphone".
   - Both: after the first buffer, show the level meter, so the person sees the feed
     is live before Sunday.
3. Never ask anywhere else. The recorder checks the status before each recording
   but never calls `requestAccess` itself. On macOS, if the status is
   `NotDetermined` when a recording should start (the input was chosen on an older
   build), it reports `PermissionDenied` instead of opening the input, because
   opening it would show the prompt in the middle of a service.

### How `PermissionDenied` is detected

- **macOS:** from the AVFoundation status only (`Denied` or `Restricted`, and
  `NotDetermined` outside Setup), checked before the stream is built and again when
  Setup gets focus. cpal will not report it, because a denied stream gives zeros.
  The 30-second "No sound is coming in" warning in decision 0012 stays as the
  backstop.
- **Windows:** from cpal's error when the stream is built:
  `BuildStreamError::BackendSpecific` whose text contains `0x80070005`, matched in
  one small function with a test on the string. Treat `DeviceNotAvailable` for an
  input that is still listed as "possibly permission" and say both reasons ("the
  input was unplugged, or Windows is not letting Lampstand use the microphone").
  Don't read the registry.
- **Linux:** no permission layer for ALSA or PulseAudio. Never reports
  `PermissionDenied`.
- `PermissionDenied` stays a bare code in `RecordNow`, as T120 says. The words and
  the button are on the device that shows them, and the button only works on the
  recorder itself.

### Files a build ticket would touch

- `app/src-tauri/Info.plist`: add `NSMicrophoneUsageDescription`, for example
  "Lampstand records the sermon from the input you choose in Setup > Recording. The
  recording stays on this computer."
- `app/src-tauri/entitlements.plist`: add `com.apple.security.device.audio-input`,
  and update the comment to say why.
- `Cargo.toml` (workspace) and `app/src-tauri/Cargo.toml`: macOS-only
  `objc2-av-foundation = "0.3"` (features for `AVCaptureDevice`, `AVMediaFormat`,
  `block2`), with `objc2` 0.6 and `block2` 0.6. They are already in `Cargo.lock`
  through Tauri. Lampstand's own `objc2` 0.5 in `display_names.rs` can stay, or move
  to 0.6 in the same change.
- `app/src-tauri/src/mic_permission.rs` (new): `status() -> Granted | NotAsked |
  Denied | Restricted` and `request()` on macOS; Windows and Linux return `Granted`.
  `settings_url()` per system.
- `app/src-tauri/src/recorder.rs` (T121): check `status()` before opening; map the
  Windows error to `PermissionDenied`.
- `app/src-tauri/src/handler/record.rs` (T121): `Request::Record` gains
  `Permission` (status) and `AskPermission` (Setup only), and `OpenPermissionSettings`
  through `obs_control::open_url` (or a shared helper moved out of it).
- `app/src/routes/setup/` (T122): the Recording page, with the ask, the reason and
  the button.
- `app/src/lib/recording.ts` (T122): the words for each problem.
- `crates/lampstand-media` if input capture lives there beside the player.

### Tests before release

- Mac, signed and notarised build, after `tccutil reset Microphone org.lampstand.app`:
  launch, play a song with sound and a video, open Setup > Sound. No prompt. Then
  choose an input in Setup > Recording: one prompt. Refuse it: Setup shows the reason
  and the button, a cue to record reports `PermissionDenied`, and nothing crashes.
- Mac build without the entitlement (to confirm the risk): choosing an input
  should end the app, as Apple says. This is why the entitlement is not optional.
- Windows, with "Let desktop apps access your microphone" off: choosing an input
  shows `PermissionDenied` and the button opens the right page. Note which cpal
  error came back, and fix the mapping if it was not `BackendSpecific`.
