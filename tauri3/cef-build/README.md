# Building our own CEF

The Chromium build (`tauri3/`) runs on the CEF binary the `cef` crate
downloads from Spotify's CDN, which is built without the H.264, AAC and MP3
decoders, so the page's `<video>` element could not play most of a library's
videos on Linux. Building CEF ourselves with those decoders on lets the player
use the page's `<video>` element on Linux as it does elsewhere. A second pass
adds H.265 (HEVC) software decoding through the patches in `patches/`, which
Chromium's own configuration never enables.

## Where it runs

A Proxmox VM, `cef-forge`: Ubuntu Server 26.04, 24 vCPU (2 x 12, type host,
NUMA), 64 GB RAM with ballooning off, 300 GB on VirtIO SCSI single with an IO
thread, a 16 GB swap file, user `develop` with passwordless sudo. The
checkout is about 40 GB and takes 1 to 2 hours; a Release build takes 2 to 4
hours on 24 threads and peaks at about 150 GB of disk. Snapshot the VM after
the checkout and after the first good build; a new CEF version is then an
incremental update, not a fresh fetch.

## Steps

The pins (`CEF_BRANCH`, `CEF_COMMIT`, `CEF_VERSION` at the top of
`build-cef.sh`) are the version the `cef` crate in
`tauri3/src-tauri/Cargo.toml` expects. Read them from the crate's own download
when the pin moves:

```bash
grep -E 'CEF_VERSION|CEF_COMMIT_HASH' tauri3/.cef/*/cef_linux_x86_64/include/cef_version.h
```

On the VM:

```bash
scp -r tauri3/cef-build/build-cef.sh tauri3/cef-build/patches cef-forge:cef/
ssh cef-forge 'cef/build-cef.sh fetch'   # once per CEF version
ssh cef-forge 'cef/build-cef.sh build'   # hours; leaves cef/out/<archive> + .archive.json
```

`build` applies `patches/` first (`build-cef.sh patch` does only that;
`unpatch` takes them out for a plain H.264 build). Run a build detached from
the SSH session, or it dies with it:

```bash
ssh cef-forge 'cd cef && CEF_OUT=$HOME/cef/out/hevc setsid nohup ./build-cef.sh build > build-hevc.log 2>&1 < /dev/null &'
```

Three things the checkout does not do by itself, which `build-cef.sh` does
before building: an official build needs Chromium's PGO profile
(`tools/update_pgo_profiles.py`, about 330 MB) and V8's builtins profile
(`v8/tools/builtins-pgo/download_profiles.py`), both from public buckets;
and `use_siso=false` keeps `autoninja` on plain ninja, since siso (Chromium's
newer runner) exits in offline mode. Switching an output directory from siso
to ninja needs `gn clean` once.

`build-cef.sh` wraps CEF's `automate-git.py` with these GN defines:
`is_official_build=true proprietary_codecs=true ffmpeg_branding=Chrome
use_sysroot=true symbol_level=0 blink_symbol_level=0 v8_symbol_level=0
use_thin_lto=true`. The first two codec flags add H.264, AAC and MP3; the rest
make the same official, symbol-free build Spotify ships.

Then on the machine that builds the launcher:

```bash
scp cef-forge:cef/out/cef_binary_*_linux64_minimal.tar.bz2* tauri3/cef-build/out/
tauri3/cef-build/install-cef.sh tauri3/cef-build/out/cef_binary_<version>_linux64_minimal.tar.bz2
```

`install-cef.sh` unpacks into `CEF_PATH/<version>/cef_linux_x86_64/` with the
`archive.json` the crate's build script checks (it compares the version in the
archive name with the crate's and does not verify the hash). Nothing in the
crate changes. Release builds download the tarball from the project's download
site (`https://downloads.dlsitearchive.com/cef/<archive>`) instead of Spotify's
CDN, check its SHA-1 and install it the same way; see the release workflow.

## Patches

Two `git apply` patches, kept as files so a new CEF version re-applies them
(or shows where they no longer fit). The file name says which repository they
belong to: `ffmpeg-*` is `third_party/ffmpeg`, which has its own git
checkout; anything else is `chromium/src`.

`ffmpeg-hevc-software-decoder.patch` turns on FFmpeg's HEVC decoder and
parser in the generated configuration Chromium builds from
(`chromium/config/Chrome/linux/x64/`): `CONFIG_HEVC_DECODER` and
`CONFIG_HEVC_PARSER` in `config_components.h` and `.asm`, the subsystems
FFmpeg's `configure` selects for them (`CONFIG_HEVCPARSE`, `CONFIG_HEVC_SEI`,
`CONFIG_BSWAPDSP`) in `config.h` and `.asm`, `&ff_hevc_decoder` and
`&ff_hevc_parser` in `codec_list.c` and `parser_list.c`, and a block at the
end of `ffmpeg_generated.gni` with the sources those objects need
(`libavcodec/hevc/`, `bswapdsp.c`, the x86 `hevc/` and `h26x/` assembly) for
`use_linux_config && current_cpu == "x64" && ffmpeg_branding == "Chrome"`.
Three of those sources share a basename with files already in the target
(`cabac.c`, `parse.c`, `parser.c`), which GN rejects, so they go in through
`autorename_libavcodec_hevc_*.c` wrappers like the `autorename_` files
Chromium generates, with the originals listed under `ffmpeg_c_deps`. The
other objects the decoder wants (`h2645_*`, `h274`, `dovi_rpu*`, `itut35`,
`aom_film_grain`) were already in for H.264.

`media-hevc-software-decoder.patch` lets the media layer use it. On Linux
`enable_platform_hevc` is already on, so demuxing, codec-string parsing and
the HEVC profile mapping exist; what stops playback is that HEVC is admitted
only through a platform (hardware) decoder. `media/base/supported_types.cc`
now counts HEVC as a built-in codec when FFmpeg video decoders are compiled
(`IsDecoderBuiltInVideoCodec`) and `IsDecoderHevcProfileSupported` accepts
Main, Main 10, Main Still Picture and RExt when the platform decoder cache
does not, which is what `canPlayType`, MediaCapabilities and the FFmpeg
demuxer consult. `media/filters/ffmpeg_video_decoder.cc` accepts the codec
(`IsCodecSupported`) and gives it the H.264 thread count (three threads at
1080p, scaled by pixels) instead of the `NOTREACHED()` it had. And
`media/ffmpeg/ffmpeg_common.cc` adds `hevc` to the FFmpeg codec allow list
(`GetAllowedVideoDecoders`, set as `codec_whitelist` on every decoder
context): without it `avcodec_open2` refuses the decoder even though it is
compiled in, and the media log only says "Cannot select FFmpegVideoDecoder
... kUnsupportedConfig".

To re-derive them at a new version: on the VM, check the same spots
(`grep -n HEVC` over the config directory, `IsDecoderHevcProfileSupported`,
`IsCodecSupported`), edit, then `git diff` in each repository (`git add -N`
the new wrapper files first).

## First build (2026-11-07)

`cef_binary_154.0.34+g14c5a08+chromium-154.0.8037.98_linux64_minimal.tar.bz2`,
170 MB, sha1 `2ec13d795c37a6942512549139cc75879c62c509`, built on cef-forge in
about 3 hours of compile (59,397 ninja steps) plus 10 minutes of link; the
checkout before it took 2 hours 15. The launcher built against it reports
`canPlayType` "probably" for H.264 (Baseline and High), AAC, MP3, VP9 and AV1,
and nothing for H.265; a 2000x1500 H.264 MP4 from the library played in the
page's `<video>` with no dropped frames, seeking included. Spotify's build of
the same version reports nothing for H.264 and AAC. `libcef.so` is 535 MB
unstripped against Spotify's 1.45 GB (symbols off), 272 MB stripped either
way.

## Second build, software H.265 (2026-11-07)

Same version, same GN defines, plus the two patches above, built as an
incremental rebuild of the first build's output directory:
`cef_binary_154.0.34+g14c5a08+chromium-154.0.8037.98_linux64_minimal.tar.bz2`,
170,016,183 bytes, sha1 `2ec13d795c37a6942512549139cc75879c62c509`. Each
rebuild took about 12 minutes on cef-forge (a few dozen ninja steps, most
of it the ThinLTO relink of `libcef.so`); a targeted
`autoninja third_party/ffmpeg media/base media/filters` beforehand caught
the GN duplicate-basename error in a minute. It took two rounds: the first
tarball (sha1 `d3b49c68870e8e2a207cab3dd9630bd326215d5e`) had the decoder
compiled in and `canPlayType` already said "probably", but playback failed
with `DECODER_ERROR_NOT_SUPPORTED` because of the codec allow list; the
DevTools `Media` domain (helper `tauri3/target/cdp-media.mjs`) showed
"Cannot select FFmpegVideoDecoder" where chrome://media-internals would.

Verified in the launcher (debug build in the tauri3 container, DevTools port
9223, no screenshots): `canPlayType` reports "probably" for
`hvc1.1.6.L93.B0`, `hev1.1.6.L93.B0`, `hvc1.2.4.L120.B0` (Main 10),
`hvc1.4.10.L120.B0` (RExt) and `video/x-matroska; codecs="hvc1..."`, with
H.264 Baseline/High, AAC, MP3, VP9 and AV1 still "probably";
`MediaCapabilities.decodingInfo` says supported and smooth, not
power-efficient. Three 8-second 1280x720 x265 samples made on the VM
(`~/cef/samples/`, copies under `tauri3/target/samples/`: Main MP4, the same
stream in MKV, Main 10 MP4) played through a `<video>` in the page
(`tauri3/target/cdp-hevc.mjs`, blob: URL since the CSP allows blob: but not
data: for media): 213 to 215 frames each in 7 seconds, 0 dropped, 0
corrupted, decoder `FFmpegVideoDecoder` (`kIsPlatformVideoDecoder` false).
One earlier run showed 3 to 4 dropped frames while the host was short of
memory; repeated runs dropped none. Not tested: a real H.265 file from the
library through the player's own path, and 4K content (software decoding;
expect several cores).

`build-cef.sh unpatch` followed by `build` reproduces the H.264-only tarball.
