import {
  Captions,
  ChevronUp,
  LoaderCircle,
  Maximize2,
  Minimize2,
  Move,
  Pause,
  Play,
  Repeat,
  Repeat1,
  RotateCcw,
  RotateCw,
  Scan,
  Shuffle,
  SkipBack,
  SkipForward,
  Video,
  Volume2,
  VolumeX,
  X,
} from "lucide-react";
import { useCallback, useEffect, useRef, useState, type CSSProperties } from "react";

import { useKeyBindings } from "../../preferences/KeyBindingsProvider";
import { ariaKeyShortcuts, bindingsForAction, formatCombo } from "../../preferences/keyBindings";
import { usePresentation } from "../../preferences/PresentationProvider";
import { VOLUME_WHEEL_STEP, clampPlaybackPosition, formatPlaybackTime } from "./audioPlayer";
import { useMediaPlayback } from "./MediaPlaybackProvider";
import { mediaItemName } from "./mediaSession";
import { assetFailureMessageKey } from "./mediaAsset";
import type { MessageKey } from "../../preferences/preferences";
import {
  SUBTITLE_BACKGROUNDS,
  SUBTITLE_COLORS,
  SUBTITLE_OPACITIES,
  SUBTITLE_SIZES,
  type SubtitleStyle,
} from "./videoPlayerModel";
import {
  formatPlaybackRate,
  MediaDockMenu,
  MediaDockMenuAction,
  MediaDockMenuGroup,
  MediaDockMenuItem,
  MediaDockMenuToggle,
  PLAYBACK_RATES,
} from "./MediaDockMenu";
import type { MediaRepeatMode } from "./types";

export function MediaDock({ onExpand }: { onExpand: (installationId: string) => void }) {
  const { t } = usePresentation();
  const playback = useMediaPlayback();
  const { session, item } = playback;
  const [openMenu, setOpenMenu] = useState<"subtitles" | "more" | null>(null);
  // The bar keeps the value under the pointer itself while pressed: the
  // player's position only follows the seek, and the press can blur the bar
  // (Chromium under GTK blurs the page on every press) before it is released.
  const [scrub, setScrub] = useState<number | null>(null);
  const pressRef = useRef({ pressed: false, moved: false, changed: false });
  const { bindings } = useKeyBindings();
  const muteBindings = bindingsForAction(bindings, "toggleMute");
  const volumeRef = useRef(playback.volume);
  volumeRef.current = playback.volume;
  const setVolume = playback.setVolume;
  // The wheel over the volume control turns it, as over the video picture. A
  // listener of its own, since React's wheel handlers cannot stop the page
  // from scrolling; attached by the ref, since the control appears only once
  // a session plays.
  const volumeControl = useCallback((control: HTMLDivElement | null) => {
    if (!control) return undefined;
    const onWheel = (event: WheelEvent) => {
      if (event.deltaY === 0) return;
      event.preventDefault();
      setVolume(volumeRef.current + (event.deltaY < 0 ? VOLUME_WHEEL_STEP : -VOLUME_WHEEL_STEP));
    };
    control.addEventListener("wheel", onWheel, { passive: false });
    return () => control.removeEventListener("wheel", onWheel);
  }, [setVolume]);
  const setSubtitleStyle = (change: Partial<SubtitleStyle>) => {
    const display = playback.videoDisplay;
    if (display) display.setSubtitleStyle({ ...display.subtitleStyle, ...change });
  };
  const playerPosition = playback.positionSeconds;
  // After a release the bar shows the chosen point until the player reports
  // it, or gives up after a while, so it does not flash the old position.
  useEffect(() => {
    if (scrub === null || pressRef.current.pressed) return;
    if (Math.abs(playerPosition - scrub) <= 0.5) {
      setScrub(null);
      return;
    }
    const timer = window.setTimeout(() => setScrub(null), 2_000);
    return () => window.clearTimeout(timer);
  }, [playerPosition, scrub]);

  if (!session || !item) return null;

  const duration = playback.durationSeconds;
  const position = clampPlaybackPosition(scrub ?? playback.positionSeconds, duration);
  const percent = duration && duration > 0 ? (position / duration) * 100 : 0;
  const bufferedPercent = duration && duration > 0
    ? Math.min(100, (playback.bufferedSeconds / duration) * 100)
    : 0;
  const index = playback.items.findIndex((candidate) => candidate.ordinal === item.ordinal);
  const isVideo = session.action === "play_video";
  const wrapsVideoPlaylist = session.action === "play_video"
    && session.repeatMode === "all"
    && playback.items.length > 1;
  const commitScrub = (value: number) => {
    if (!pressRef.current.changed) return;
    pressRef.current.changed = false;
    setScrub(value);
    playback.seek(value);
  };
  // In fullscreen the dock lies over the picture and leaves with the player's
  // controls.
  const overlay = isVideo && playback.videoDisplay?.fullscreen === true;
  const hidden = overlay && !playback.videoDisplay?.controlsVisible;
  return (
    <section
      className={`media-dock${isVideo ? " is-video" : ""}${overlay ? " is-overlay" : ""}${hidden ? " is-hidden" : ""}`}
      aria-label={t("media.dock")}
    >
      <div className="media-dock-now">
        <span className="media-dock-art" aria-hidden="true">
          {session.action === "play_video"
            ? <Video fill="currentColor" />
            : playback.loading
              ? <LoaderCircle className="app-spin" />
              : <Play fill="currentColor" />}
        </span>
        <span className="media-dock-copy">
          <strong title={mediaItemName(item)}>{mediaItemName(item)}</strong>
          <small>{t(session.action === "play_video" ? "media.videoPosition" : "media.trackPosition", {
            position: index + 1,
            total: playback.items.length,
          })}</small>
        </span>
      </div>

      <div className="media-dock-center">
        <div className="media-dock-transport">
          {isVideo ? null : (
            <button
              type="button"
              aria-pressed={session.shuffle}
              title={t("media.shuffle")}
              aria-label={t("media.shuffle")}
              onClick={() => playback.setShuffle(!session.shuffle)}
            >
              <Shuffle aria-hidden="true" />
            </button>
          )}
          <button
            type="button"
            title={t("media.previous")}
            aria-label={t("media.previous")}
            disabled={playback.items.length <= 1 || (index <= 0 && !wrapsVideoPlaylist)}
            onClick={() => playback.step(-1)}
          >
            <SkipBack fill="currentColor" aria-hidden="true" />
          </button>
          <button
            type="button"
            title={t("media.skipBackward")}
            aria-label={t("media.skipBackward")}
            onClick={() => playback.skip(-15)}
          >
            <RotateCcw aria-hidden="true" />
          </button>
          <button
            className="media-dock-play"
            type="button"
            aria-label={t(playback.loading && isVideo
              ? "media.video.loading"
              : playback.playing
                ? "media.pause"
                : "media.play")}
            disabled={playback.loading}
            onClick={playback.toggle}
          >
            {playback.loading
              ? <LoaderCircle className="app-spin" aria-hidden="true" />
              : playback.playing
                ? <Pause fill="currentColor" aria-hidden="true" />
                : <Play fill="currentColor" aria-hidden="true" />}
          </button>
          <button
            type="button"
            title={t("media.skipForward")}
            aria-label={t("media.skipForward")}
            onClick={() => playback.skip(15)}
          >
            <RotateCw aria-hidden="true" />
          </button>
          <button
            type="button"
            title={t("media.next")}
            aria-label={t("media.next")}
            disabled={playback.items.length <= 1
              || (index >= playback.items.length - 1 && !wrapsVideoPlaylist)}
            onClick={() => playback.step(1)}
          >
            <SkipForward fill="currentColor" aria-hidden="true" />
          </button>
          {isVideo ? null : (
            <button
              type="button"
              aria-pressed={session.repeatMode !== "off"}
              title={t(repeatLabelKey(session.repeatMode))}
              aria-label={t(repeatLabelKey(session.repeatMode))}
              onClick={() => playback.setRepeatMode(nextRepeat(session.repeatMode))}
            >
              {session.repeatMode === "one"
                ? <Repeat1 aria-hidden="true" />
                : <Repeat aria-hidden="true" />}
            </button>
          )}
        </div>
        <div className="media-dock-scrub">
          <span>{formatPlaybackTime(position)}</span>
          <input
            className="media-dock-rail"
            type="range"
            min={0}
            max={duration ?? 0}
            step={0.1}
            value={position}
            disabled={!duration || duration <= 0}
            aria-label={t("media.seek")}
            aria-valuetext={formatPlaybackTime(position)}
            style={{
              "--media-dock-progress": `${percent}%`,
              "--media-dock-buffered": `${bufferedPercent}%`,
            } as CSSProperties}
            onPointerDown={() => {
              pressRef.current = { pressed: true, moved: false, changed: false };
            }}
            onPointerMove={() => {
              if (pressRef.current.pressed) pressRef.current.moved = true;
            }}
            onPointerCancel={() => {
              pressRef.current.pressed = false;
              setScrub(null);
            }}
            onInput={(event) => {
              const value = Number(event.currentTarget.value);
              pressRef.current.changed = true;
              setScrub(value);
              if (pressRef.current.pressed && pressRef.current.moved) playback.seek(value, false);
            }}
            onPointerUp={(event) => {
              pressRef.current.pressed = false;
              commitScrub(Number(event.currentTarget.value));
            }}
            onKeyUp={(event) => commitScrub(Number(event.currentTarget.value))}
            onBlur={(event) => {
              if (!pressRef.current.pressed) commitScrub(Number(event.currentTarget.value));
            }}
          />
          <span>{formatPlaybackTime(duration)}</span>
        </div>
      </div>

      <div className="media-dock-actions">
        {playback.videoDisplay?.subtitles.length ? (
          <MediaDockMenu
            active={playback.videoDisplay.subtitleTrack !== null}
            icon={<Captions aria-hidden="true" />}
            label={t("media.video.subtitles")}
            open={openMenu === "subtitles"}
            gap={isVideo ? -16 : 8}
            onOpenChange={(open) => setOpenMenu(open ? "subtitles" : null)}
          >
            <MediaDockMenuGroup labelKey="media.video.subtitles">
              <MediaDockMenuItem
                active={playback.videoDisplay.subtitleTrack === null}
                label={t("media.video.subtitlesOff")}
                onSelect={() => playback.videoDisplay?.setSubtitle(null)}
              />
              {playback.videoDisplay.subtitles.map((subtitle) => (
                <MediaDockMenuItem
                  active={playback.videoDisplay?.subtitleTrack === subtitle.index}
                  key={`${subtitle.index}:${subtitle.label}`}
                  label={subtitle.label}
                  onSelect={() => playback.videoDisplay?.setSubtitle(subtitle.index)}
                />
              ))}
            </MediaDockMenuGroup>
            <MediaDockMenuGroup labelKey="media.video.subtitleSize">
              {SUBTITLE_SIZES.map((size) => (
                <MediaDockMenuItem
                  active={playback.videoDisplay?.subtitleStyle.size === size}
                  key={size}
                  keepOpen
                  label={`${Math.round(size * 100)}%`}
                  onSelect={() => setSubtitleStyle({ size })}
                />
              ))}
            </MediaDockMenuGroup>
            <MediaDockMenuGroup labelKey="media.video.subtitleColor">
              {SUBTITLE_COLORS.map((color) => (
                <MediaDockMenuItem
                  active={playback.videoDisplay?.subtitleStyle.color === color}
                  key={color}
                  keepOpen
                  label={t(subtitleColorKey(color))}
                  swatch={SUBTITLE_SWATCHES[color]}
                  onSelect={() => setSubtitleStyle({ color })}
                />
              ))}
            </MediaDockMenuGroup>
            <MediaDockMenuGroup labelKey="media.video.subtitleOpacity">
              {SUBTITLE_OPACITIES.map((opacity) => (
                <MediaDockMenuItem
                  active={playback.videoDisplay?.subtitleStyle.opacity === opacity}
                  key={opacity}
                  keepOpen
                  label={`${Math.round(opacity * 100)}%`}
                  onSelect={() => setSubtitleStyle({ opacity })}
                />
              ))}
            </MediaDockMenuGroup>
            <MediaDockMenuGroup labelKey="media.video.subtitleBackground">
              {SUBTITLE_BACKGROUNDS.map((background) => (
                <MediaDockMenuItem
                  active={playback.videoDisplay?.subtitleStyle.background === background}
                  key={background}
                  keepOpen
                  label={background === 0 ? t("media.video.subtitleBackgroundNone") : `${Math.round(background * 100)}%`}
                  onSelect={() => setSubtitleStyle({ background })}
                />
              ))}
            </MediaDockMenuGroup>
          </MediaDockMenu>
        ) : null}
        <MediaDockMenu
          label={t("media.menu.more")}
          open={openMenu === "more"}
          gap={isVideo ? -16 : 8}
          onOpenChange={(open) => setOpenMenu(open ? "more" : null)}
        >
          <MediaDockMenuGroup labelKey="media.menu.speed">
            {PLAYBACK_RATES.map((rate) => (
              <MediaDockMenuItem
                active={playback.playbackRate === rate}
                key={rate}
                label={formatPlaybackRate(rate)}
                onSelect={() => playback.setPlaybackRate(rate)}
              />
            ))}
          </MediaDockMenuGroup>
          {playback.videoDisplay ? (
            <MediaDockMenuGroup labelKey="media.video.fit">
              <MediaDockMenuItem
                active={playback.videoDisplay.fit !== "cover"}
                label={t("media.video.fitContain")}
                onSelect={() => playback.videoDisplay?.setFit("contain")}
              />
              <MediaDockMenuItem
                active={playback.videoDisplay.fit === "cover"}
                label={t("media.video.fitCover")}
                onSelect={() => playback.videoDisplay?.setFit("cover")}
              />
            </MediaDockMenuGroup>
          ) : null}
          {playback.videoDisplay ? (
            <MediaDockMenuToggle
              label={t("media.video.backgroundAmbient")}
              checked={playback.videoDisplay.ambient}
              onToggle={(enabled) => playback.videoDisplay?.setAmbient(enabled)}
            />
          ) : null}
          {isVideo ? (
            <MediaDockMenuGroup labelKey="media.menu.repeat">
              {(["off", "all", "one"] as const).map((mode) => (
                <MediaDockMenuItem
                  active={session.repeatMode === mode}
                  key={mode}
                  label={t(repeatLabelKey(mode))}
                  onSelect={() => playback.setRepeatMode(mode)}
                />
              ))}
            </MediaDockMenuGroup>
          ) : null}
          {playback.videoDisplay ? (
            <MediaDockMenuAction
              label={t(playback.videoDisplay.fullscreen ? "media.video.exitFullscreen" : "media.video.fullscreen")}
              onSelect={() => playback.videoDisplay?.toggleFullscreen()}
            />
          ) : null}
          {playback.videoDisplay?.canPictureInPicture ? (
            <MediaDockMenuAction
              label={t("media.video.pictureInPicture")}
              onSelect={() => playback.videoDisplay?.togglePictureInPicture()}
            />
          ) : null}
        </MediaDockMenu>
        <div className="media-dock-volume" ref={volumeControl}>
          <span className="media-dock-hint-anchor">
            <button
              type="button"
              className="media-dock-volume-toggle"
              aria-label={t(playback.volume > 0 ? "media.video.mute" : "media.video.unmute")}
              aria-keyshortcuts={ariaKeyShortcuts(muteBindings)}
              aria-pressed={playback.volume <= 0}
              onClick={() => playback.toggleMute()}
            >
              {playback.volume > 0 ? <Volume2 aria-hidden="true" /> : <VolumeX aria-hidden="true" />}
            </button>
            {/* The hover hint YouTube shows: the action in the user's language and its key. */}
            <span className="media-dock-hint" aria-hidden="true">
              {t(playback.volume > 0 ? "media.video.mute" : "media.video.unmute")}
              {muteBindings[0] ? <kbd>{formatCombo(muteBindings[0])}</kbd> : null}
            </span>
          </span>
          <input
            type="range"
            min={0}
            max={1}
            step={0.01}
            value={playback.volume}
            aria-label={t("media.volume")}
            onChange={(event) => playback.setVolume(Number(event.target.value))}
          />
        </div>
        {playback.videoDisplay ? (
          <button
            type="button"
            title={t(playback.videoDisplay.fullscreen ? "media.video.exitFullscreen" : "media.video.fullscreen")}
            aria-label={t(playback.videoDisplay.fullscreen ? "media.video.exitFullscreen" : "media.video.fullscreen")}
            onClick={playback.videoDisplay.toggleFullscreen}
          >
            {playback.videoDisplay.fullscreen
              ? <Minimize2 aria-hidden="true" />
              : <Maximize2 aria-hidden="true" />}
          </button>
        ) : null}
        <button
          type="button"
          title={t("media.openFullPlayer")}
          aria-label={t("media.openFullPlayer")}
          onClick={() => onExpand(session.installationId)}
        >
          <ChevronUp aria-hidden="true" />
        </button>
        <button
          type="button"
          title={t("media.closePlayer")}
          aria-label={t("media.closePlayer")}
          onClick={playback.close}
        >
          <X aria-hidden="true" />
        </button>
      </div>

      {playback.failure || playback.videoDisplay?.subtitleError
        ? <p className="media-dock-error" role="alert">{t(playback.failure
          ? assetFailureMessageKey(playback.failure)
          : "media.video.subtitleFailed")}</p>
        : null}
    </section>
  );
}

const SUBTITLE_SWATCHES: Record<SubtitleStyle["color"], string> = {
  white: "#ffffff",
  yellow: "#ffe44d",
  cyan: "#7fe9ff",
};

function subtitleColorKey(color: SubtitleStyle["color"]): MessageKey {
  if (color === "yellow") return "media.video.subtitleColorYellow";
  if (color === "cyan") return "media.video.subtitleColorCyan";
  return "media.video.subtitleColorWhite";
}

export function nextRepeat(mode: MediaRepeatMode): MediaRepeatMode {
  if (mode === "off") return "all";
  if (mode === "all") return "one";
  return "off";
}

function repeatLabelKey(mode: MediaRepeatMode) {
  if (mode === "all") return "media.repeatAll" as const;
  if (mode === "one") return "media.repeatOne" as const;
  return "media.repeatOff" as const;
}
