# Chromium build (the Linux release build)

The launcher from `tauri2/`, built with Tauri 3 (alpha) and its CEF runtime,
so that on Linux it runs on Chromium instead of WebKitGTK. Windows keeps the
Tauri 2 build: WebView2 is Chromium already.

It started as a prototype for comparing the two on Linux; since 0.1.0-alpha.5
it is the build the project publishes for Linux, as the deb `dla-launcher`
(product name "DLA Launcher", like the WebKitGTK deb it replaces in place). The
WebKitGTK build of `tauri2/` stays buildable as a fallback.

## What is in this folder

Only what has to differ. The app crate builds `tauri2/src-tauri`'s code and
build script with its `chromium` feature, which:

- selects the CEF runtime (`tauri2/src-tauri/src/chromium.rs`);
- removes the work source of Chromium's GLib message pump from the main loop at
  startup: CEF's external pump never runs that pump, so the source asks GLib
  for a zero poll timeout on every pass and the main thread would spin a core
  at 100% (`remove_idle_glib_work_source` in `chromium.rs`);
- leaves out the WebKitGTK and GTK 3 code: the render recovery (the page
  installs its render watchdog only on WebKitGTK), the media scheme
  registration and the GTK window reveal; SIGINT and SIGTERM end both builds
  cleanly;
- plays video and subtitles from `http://<scheme>.localhost` URLs, as on
  Windows, because Chromium gives each scheme an http origin and does not load
  `file:` URLs into those pages.

The CEF runtime itself is built from a vendored copy with the launcher's
patches (`patches/tauri-runtime-cef-*.patch`, applied into the gitignored
`vendor/` by `scripts/prepare-runtime-cef.sh`, which `Cargo.toml` points at
with `[patch.crates-io]`): the released runtime's resource handler cannot skip
to a requested byte range, so every `Range` request past byte 0 failed and
audio could not seek; and its X11 host only follows the window through GDK's
layout, which on mutter once kept a maximized window's old size after it went
fullscreen, leaving a band at the bottom where the page showed through the
video (the second patch sizes the host from the X server when GDK disagrees
for longer than half a second, and logs it). Both are proposed upstream.

The UI is `tauri2/ui` built with `DLA_WEBVIEW=chromium` into `ui-dist/`. Video
plays in the page's own `<video>` element on the CEF the project builds itself
with the H.264/AAC decoders (`cef-build/`); the build tells the page so at
startup (`read_video_backend`).

The CEF runtime uses GTK 4, which cannot share a process with GTK 3, so the
dialog plugin uses the desktop portal (`xdg-portal`) for folder pickers instead
of its GTK 3 dialogs.

`crates/dla-android-package-tauri` builds the plugin of the same name from
`tauri2/` against Tauri 3. Every other crate is used as it is. Both builds
have the same identifier, so they open the same library; only one runs at a
time.

## Build and run on Linux

Install the prerequisites from [Building](../docs/BUILDING.md#linux-chromium-build)
(GTK 4.14 or newer: Ubuntu 24.04, Debian 13, Fedora 40 or later), the frontend
dependencies and the sidecars, then from the repository root:

```bash
bash tauri3/scripts/prepare-runtime-cef.sh
export CEF_PATH="$PWD/tauri3/.cef"
mkdir -p "$CEF_PATH"
curl -fLO https://downloads.dlsitearchive.com/cef/cef_binary_154.0.34+g14c5a08+chromium-154.0.8037.98_linux64_minimal.tar.bz2
echo "2ec13d795c37a6942512549139cc75879c62c509  cef_binary_154.0.34+g14c5a08+chromium-154.0.8037.98_linux64_minimal.tar.bz2" | sha1sum -c
bash tauri3/cef-build/install-cef.sh cef_binary_154.0.34+g14c5a08+chromium-154.0.8037.98_linux64_minimal.tar.bz2
cd tauri3
DLA_WEBVIEW=chromium pnpm --dir ../tauri2/ui exec vite build --mode production --outDir ../../tauri3/ui-dist --emptyOutDir
cargo build --features tauri/custom-protocol
./target/debug/dla-launcher-chromium
```

The first step vendors the patched CEF runtime (above); without it cargo
stops at the missing `vendor/tauri-runtime-cef` path. The next ones install
the project's CEF distribution (`cef-build/`, 170 MB) where the `cef` crate
looks; without `CEF_PATH` the crate downloads Spotify's build of the same
version (about 1 GB), which has no H.264 or AAC decoder, so most videos would
not play. Both go into `tauri3/.cef`, which is gitignored. In an unprivileged
container Chromium's sandbox cannot start; set `DLA_CHROMIUM_DISABLE_SANDBOX=1`
there. Packaged builds keep it.

## Packaging

```bash
bash tauri3/scripts/install-tauri-cli.sh
cd tauri3
cargo tauri build --ci --bundles deb
```

The Tauri 3 CLI (3.0.0-alpha.5, its bundler `tauri-bundler` 3.0.0-alpha.4)
builds the deb. The script installs it from source with the bundler patched
(`patches/`): the released bundler copies `libEGL.so` and `libGLESv2.so`,
which CEF 154 no longer ships on Linux, and stops when they are missing. The
prebuilt `@tauri-apps/cli@3.0.0-alpha.5` from npm fails the same way.

- It copies the CEF distribution next to the executable and strips every
  shared library: `libcef.so` is 535 MB with symbols and 272 MB without.
  Only Chromium's `en-US` language packs ship. SwiftShader and ANGLE (software
  GL for machines without a usable GPU driver), the Vulkan loader and
  `chrome-sandbox` (installed setuid root in the deb) ship too.
- In the deb the executable moves to `/usr/share/DLA Launcher/` with the
  CEF files, and `/usr/bin/dla-launcher-chromium` is a symlink to it. The
  sidecars (`dla-cloudflared`, `dla-7zip`) stay in `/usr/bin`, so the launcher
  falls back to looking them up on `PATH` when they are not beside it.
- The bundler computes no library dependencies for a CEF app, so the deb's
  `Depends` are declared in `src-tauri/tauri.conf.json`: Chromium's host
  libraries (from `ldd libcef.so`, the same set Chromium's own deb declares)
  and GTK 4.
- The CLI appends `libgtk-3-0` to the depends of every Linux deb; harmless here.
- An AppImage builds too (`--bundles appimage`, through quick-sharun as pinned
  by the bundler) but is not part of the release. Without a terminal set
  `STRACE_MODE=0`, and set
  `ANYLINUX_LIB_SOURCE=https://raw.githubusercontent.com/pkgforge-dev/Anylinux-sharun/refs/heads/main/lib/anylinux.c`
  until the pinned fork follows upstream's move of that file.

## Known differences

- Tauri 3 and the CEF runtime are alphas; versions are pinned exactly.
- It needs GTK 4.14 or newer: Ubuntu 24.04, Debian 13, Fedora 40 or later.
- Chromium ships inside the app, so packages are hundreds of MB larger.
- Only Chromium's `en-US` language pack is bundled, for Chromium's own text.
- Chromium's Safe Browsing, component updater and spell checker are off: the
  page only loads the launcher's own files, and every remote request goes
  through Rust. Playback may start without a click (autoplay allowed). The
  launcher's processes open no socket off loopback, idle and while playing
  audio and reading a PDF.
- A development build uses its own Chromium profile, `cef-dev` beside the
  release build's `cef` under `~/.cache/org.dlaproject.launcher/`: a release
  run writes `devtools.remote_debugging.allowed = false` into the profile it
  opens, which would otherwise close the development build's DevTools port
  (`DLA_CHROMIUM_DEVTOOLS_PORT`).
- Audio streams from the media protocol (`audioDelivery: "stream"`, no blob
  per track) and the reader reads a PDF by byte ranges, as on Windows: CEF
  slices a range request by skipping through the scheme handler's body, which
  the vendored runtime's patch implements. WebKitGTK keeps the blob.
- The HTML Fullscreen API takes the window fullscreen and gives it back when
  the page leaves, as on WebKitGTK and WebView2: `tauri-runtime-cef`
  3.0.0-alpha.7 mirrors the page's fullscreen state to the window.
- CEF's standard builds leave out the H.264 and AAC decoders; the project's
  own build (`cef-build/`) has them, so video plays in the page and the deb
  needs no GStreamer. H.265 waits for a second pass of that build; such files
  keep the external-player option.
