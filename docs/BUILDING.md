# Building DLA Launcher

Build on the operating system you intend to package. Windows MSI and NSIS
installers should be produced on Windows; Linux bundles should be produced on a
compatible Linux distribution. Cross-compilation is not a substitute for a
native runtime check.

## Pinned tools

| Tool | Version |
| --- | --- |
| Rust | 1.97.1 (from `rust-toolchain.toml`) |
| Node.js | 24 LTS |
| pnpm | 11.21.0 (from `package.json`) |
| Tauri CLI | 2.11.4 |
| Android compile/target SDK | 36 |
| Android NDK | 29.0.14206865 |
| Android Java | 17 |

Enable pnpm through Corepack and install the pinned frontend dependencies:

```bash
corepack enable
pnpm install --frozen-lockfile
cargo install tauri-cli --version 2.11.4 --locked
```

## Linux (Debian or Ubuntu)

Install Tauri's native requirements and 7-Zip for RAR inspection/extraction:

```bash
sudo apt update
sudo apt install \
  libwebkit2gtk-4.1-dev \
  build-essential \
  curl \
  wget \
  file \
  libxdo-dev \
  libssl-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev \
  pkg-config \
  gstreamer1.0-gl \
  gstreamer1.0-libav \
  gstreamer1.0-plugins-bad \
  gstreamer1.0-plugins-base \
  gstreamer1.0-plugins-good \
  gstreamer1.0-plugins-ugly \
  gstreamer1.0-tools \
  p7zip-full
```

Then run or package the application:

```bash
cd tauri2
cargo tauri dev
cargo tauri build --ci
```

Release Linux bundles are built against the Debian 12 / Ubuntu 22.04 x86_64
compatibility baseline. The platform-specific Tauri configuration produces DEB
and RPM packages. Wayland and X11 are the tested display systems. RPM artifacts
target other modern distributions on a best-effort basis until a native runtime
gate is recorded for that distribution. Headless and direct-framebuffer
environments are not supported.

Do not set `GDK_BACKEND` in a package or desktop entry. GTK selects the native
session backend, and WebKitGTK GPU compositing must remain enabled by default.
Backend overrides and `WEBKIT_DISABLE_COMPOSITING_MODE=1` are troubleshooting
measures, not supported release defaults.

The DEB package recommends the GStreamer plugin families used by the built-in
audio and video players, plus 7-Zip for RAR extraction. RPM users must install
the equivalent packages supplied by their distribution.

AppImage is deliberately excluded from the default Linux targets. With Tauri
2.11.5, enabling `bundleMediaFramework` copies build-host GLib, Wayland, and
GStreamer libraries into the artifact. On a newer Mesa/GLib host this currently
causes an EGL failure and a blank WebKit window, matching
[tauri-apps/tauri#15665](https://github.com/tauri-apps/tauri/issues/15665).
Replacing those libraries with host files makes one machine work but is not a
portable package, so do not publish a manually post-processed AppImage.

AppImage can return to the release targets after Tauri exposes a supported
library-exclusion fix and one artifact passes startup plus representative audio
and video playback on both the oldest supported baseline and a current
Wayland/Mesa system. If media libraries are bundled again, retain the copyright
and license information for every component; `plugins-ugly` and `libav` require
an explicit redistribution review.

Wine is optional and is required only to launch reviewed Windows executables
on Linux. A functioning 32-bit Wine environment is required for 32-bit titles.

## Windows 11

Install:

1. Git for Windows.
2. Microsoft Visual Studio 2022 Build Tools with **Desktop development with
   C++**.
3. Microsoft Edge WebView2 Runtime if it is not already present.
4. Rustup with the `x86_64-pc-windows-msvc` host.
5. Node.js 24 LTS and Corepack.
6. 7-Zip.

In PowerShell:

```powershell
rustup toolchain install 1.97.1-x86_64-pc-windows-msvc `
  --profile minimal `
  --component rustfmt `
  --component clippy
rustup override set 1.97.1-x86_64-pc-windows-msvc
corepack enable
pnpm install --frozen-lockfile
cargo install tauri-cli --version 2.11.4 --locked
$env:DLA_ARCHIVE_TOOL = "C:\Program Files\7-Zip\7z.exe"
Set-Location tauri2
cargo tauri build --ci
```

The packages are written below `tauri2/target/release/bundle`. The default
machine-wide installation is `C:\Program Files\DLA Launcher`. If MSI creation
fails while running WiX `light.exe`, confirm that Windows' optional VBSCRIPT
feature is enabled.

## Android

Install Android Studio or equivalent command-line SDK tooling with JDK 17,
platform 36, build tools 36.0.0 and 35.0.0, NDK 29.0.14206865, and platform
tools. Set `JAVA_HOME`, `ANDROID_HOME`, and `NDK_HOME`, then add the Rust targets:

```bash
rustup target add \
  aarch64-linux-android \
  armv7-linux-androideabi \
  i686-linux-android \
  x86_64-linux-android
```

The checked-in Android project contains the launcher-owned Kotlin integration.
Tauri generates its machine-specific Gradle glue as part of the first Android
build. From a clean clone, build a debug APK before invoking Gradle directly:

```bash
cd tauri2
cargo tauri android build --debug --apk --target x86_64 aarch64 --ci
```

After that build succeeds, run Kotlin unit tests and compile the two synthetic
test applications:

```bash
cd tauri2/src-tauri/gen/android
./gradlew testDebugUnitTest
./gradlew -p ../../../tests/android-apk-fixture assembleDebug
./gradlew -p ../../../tests/android-launch-fixture assembleDebug
```

### Signed Android releases

Copy `tauri2/src-tauri/gen/android/keystore.properties.example` to
`keystore.properties` and point it at an owner-managed keystore outside Git.
Never use a disposable development key for production.

With `apkanalyzer`, `bundletool`, `jarsigner`, `keytool`, `zipalign`, and
`apksigner` available:

```bash
cd tauri2
bash scripts/build-android-release.sh
```

The script validates signatures, package identity, SDK levels, ABI contents,
alignment, size budgets, mappings, and checksums. A release still needs a clean
emulator or trusted-device install, launch, and same-key upgrade check.

## Archive tool override

DLA Launcher discovers `7z`, `7zz`, or `7z.exe`. Set an explicit executable
when it is installed elsewhere:

```bash
export DLA_ARCHIVE_TOOL=/absolute/path/to/7zz
```

PowerShell:

```powershell
[Environment]::SetEnvironmentVariable(
  "DLA_ARCHIVE_TOOL",
  "C:\Program Files\7-Zip\7z.exe",
  "User"
)
```

ZIP support is built in. The external tool is used only for supported RAR
inspection and extraction and is never used to execute archive contents.
