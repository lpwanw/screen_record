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

## Signing identity
TCC keys permissions on the signing certificate; ad-hoc signatures change every build and lose grants. Create a self-signed code-signing identity once:

```sh
openssl req -x509 -newkey rsa:2048 -nodes -keyout key.pem -out cert.pem -days 3650 -subj "/CN=Taywind Dev" \
  -addext "basicConstraints=critical,CA:false" -addext "keyUsage=critical,digitalSignature" \
  -addext "extendedKeyUsage=critical,codeSigning"
openssl pkcs12 -export -legacy -inkey key.pem -in cert.pem -out id.p12 -passout pass:taywind
security import id.p12 -k ~/Library/Keychains/login.keychain-db -P taywind -T /usr/bin/codesign
rm key.pem cert.pem id.p12
security find-identity -p codesigning   # lists "Taywind Dev" (CSSMERR_TP_NOT_TRUSTED is fine)
```

Reset permissions if needed: `tccutil reset All dev.taywind.record`.
