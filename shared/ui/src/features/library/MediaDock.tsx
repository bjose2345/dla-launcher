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
  X,
} from "lucide-react";
import { useState, type CSSProperties } from "react";

import { usePresentation } from "../../preferences/PresentationProvider";
import { clampPlaybackPosition, formatPlaybackTime } from "./audioPlayer";
import { useMediaPlayback } from "./MediaPlaybackProvider";
import { mediaItemName } from "./mediaSession";
import { assetFailureMessageKey } from "./mediaAsset";
import {
  formatPlaybackRate,
  MediaDockMenu,
  MediaDockMenuGroup,
  MediaDockMenuItem,
  PLAYBACK_RATES,
} from "./MediaDockMenu";
import type { MediaRepeatMode } from "./types";

export function MediaDock({ onExpand }: { onExpand: (installationId: string) => void }) {
  const { t } = usePresentation();
  const playback = useMediaPlayback();
  const { session, item } = playback;
  const [openMenu, setOpenMenu] = useState<"subtitles" | "more" | null>(null);

  if (!session || !item) return null;

  const duration = playback.durationSeconds;
  const position = clampPlaybackPosition(playback.positionSeconds, duration);
  const percent = duration && duration > 0 ? (position / duration) * 100 : 0;
  const bufferedPercent = duration && duration > 0
    ? Math.min(100, (playback.bufferedSeconds / duration) * 100)
    : 0;
  const index = playback.items.findIndex((candidate) => candidate.ordinal === item.ordinal);
  const isVideo = session.action === "play_video";
  const wrapsVideoPlaylist = session.action === "play_video"
    && session.repeatMode === "all"
    && playback.items.length > 1;
  return (
    <section
      className={`media-dock${isVideo ? " is-video" : ""}`}
      aria-label={t("media.dock")}
    >
      <div className="media-dock-now">
        <span className="media-dock-art" aria-hidden="true">
          {session.action === "play_video"
            ? <Video fill="currentColor" />
            : playback.loading
              ? <LoaderCircle className="library-spin" />
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
              ? <LoaderCircle className="library-spin" aria-hidden="true" />
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
            onInput={(event) => playback.seek(Number(event.currentTarget.value), false)}
            onPointerUp={(event) => playback.seek(Number(event.currentTarget.value))}
            onKeyUp={(event) => playback.seek(Number(event.currentTarget.value))}
            onBlur={(event) => playback.seek(Number(event.currentTarget.value))}
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
                active={playback.videoDisplay.fit === "contain"}
                label={t("media.video.fitContain")}
                onSelect={() => playback.videoDisplay?.setFit("contain")}
              />
              <MediaDockMenuItem
                active={playback.videoDisplay.fit === "cover"}
                label={t("media.video.fitCover")}
                onSelect={() => playback.videoDisplay?.setFit("cover")}
              />
              <MediaDockMenuItem
                active={playback.videoDisplay.fit === "original"}
                label={t("media.video.fitOriginal")}
                onSelect={() => playback.videoDisplay?.setFit("original")}
              />
            </MediaDockMenuGroup>
          ) : null}
          {isVideo ? (
            <MediaDockMenuGroup labelKey="media.menu.playback">
              <MediaDockMenuItem
                active={session.repeatMode !== "off"}
                label={t(repeatLabelKey(session.repeatMode))}
                onSelect={() => playback.setRepeatMode(nextRepeat(session.repeatMode))}
              />
              {playback.videoDisplay ? (
                <MediaDockMenuItem
                  active={false}
                  label={t("media.video.pictureInPicture")}
                  onSelect={() => playback.videoDisplay?.togglePictureInPicture()}
                />
              ) : null}
            </MediaDockMenuGroup>
          ) : null}
        </MediaDockMenu>
        <label className="media-dock-volume">
          <Volume2 aria-hidden="true" />
          <input
            type="range"
            min={0}
            max={1}
            step={0.01}
            value={playback.volume}
            aria-label={t("media.volume")}
            onChange={(event) => playback.setVolume(Number(event.target.value))}
          />
        </label>
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
