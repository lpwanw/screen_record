# Development

macOS 15+ only. Toolchain is pinned in `rust-toolchain.toml` (rustup installs it on first build).

## Requirements
- Xcode Command Line Tools (`xcode-select --install`); full Xcode not needed (`runtime_shaders` + `build.rs` locate the Swift runtime).
- `ffmpeg` (Homebrew) for export.
- Code signing identity `Taywind Dev` (below).

## Run
```sh
./scripts/bundle.sh
```
Builds, bundles `dist/TaywindRecord.app` (Info.plist with camera/mic/screen usage strings), signs it, kills any running instance and relaunches via `open`. Logs: `~/Library/Logs/TaywindRecord.log`.

Always launch the `.app` (not `cargo run`): macOS privacy permissions (TCC) attach to the signed bundle. Recordings go to `~/Movies/Taywind/`.

`codesign` needs the Mac unlocked; the first run after creating the identity asks for keychain access (choose "Always Allow").

## Share as DMG
```sh
./scripts/bundle.sh --dmg
```
Writes `dist/TaywindRecord.dmg` (release build, Apple Silicon only, signed with `Taywind Dev`, not notarized). Keep signing with the same identity so recipients keep their permission grants across updates.

Recipients:
1. Open the DMG and drag Taywind Record to Applications.
2. Launch it once; macOS blocks it. Open System Settings → Privacy & Security → **Open Anyway** (or run `xattr -dr com.apple.quarantine /Applications/TaywindRecord.app`).
3. `brew install ffmpeg` for export.

## Using the app
1. First launch asks for Screen Recording (relaunch after granting), Camera and Microphone.
2. Pick **Display**, **Window** (Refresh lists windows) or **Region** (Select region… then drag; Esc or right-click cancels).
3. Pick camera and microphone (or None) and the System audio switch. The floating bubble shows the camera, or a live mic waveform when there is no camera; drag it where it should appear in the video.
4. **Record** / **Stop**. The panel and bubble are never captured; the bubble position at Stop is stored for export.
5. **Export MP4** composites the tracks into `export.mp4` (progress bar); **Open folder** reveals it. Closing the panel while recording saves the recording first.

Each recording folder `~/Movies/Taywind/<YYYY-MM-DD HH-MM-SS>/` holds separate tracks for later editing:

| File | Content |
|---|---|
| `screen.mp4` | HEVC screen capture (+ system audio track) |
| `camera.mov` | Camera, if one was selected |
| `mic.wav` | Microphone, 32-bit float |
| `project.json` | Mode, capture rect, pixel size, duration, per-track host-clock start, normalized bubble position |
| `export.mp4`, `export.log` | Export output and the ffmpeg log |

## Tests
```sh
cargo test                  # project model, ffmpeg args, mic writer
cargo test -- --ignored     # end-to-end export on synthetic inputs (needs ffmpeg + VideoToolbox)
cargo clippy --all-targets
```

## Signing identity
TCC keys permissions on the signing certificate; ad-hoc signatures change every build and lose grants. Create a self-signed code-signing identity once:

```sh
openssl req -x509 -newkey rsa:2048 -nodes -keyout key.pem -out cert.pem -days 3650 -subj "/CN=Taywind Dev" \
  -addext "basicConstraints=critical,CA:false" -addext "keyUsage=critical,digitalSignature" \
  -addext "extendedKeyUsage=critical,codeSigning"
P12_PASS=$(openssl rand -hex 16)
openssl pkcs12 -export -legacy -inkey key.pem -in cert.pem -out id.p12 -passout "pass:$P12_PASS"
security import id.p12 -k ~/Library/Keychains/login.keychain-db -P "$P12_PASS" -T /usr/bin/codesign
rm key.pem cert.pem id.p12; unset P12_PASS
security find-identity -p codesigning   # lists "Taywind Dev" (CSSMERR_TP_NOT_TRUSTED is fine)
```

Reset permissions if needed: `tccutil reset All dev.taywind.record`.
