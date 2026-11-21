import { AudioLines, Headphones, Pause } from "lucide-react";
import { useEffect, useRef, useState, type RefObject } from "react";

import { BackButton } from "../../app/BackButton";
import { isInteractiveTarget } from "../../preferences/keyBindings";
import { usePresentation } from "../../preferences/PresentationProvider";
import type { CatalogWork } from "../catalog";
import { VOLUME_WHEEL_STEP } from "./audioPlayer";
import { LiveVisual } from "./AudioVisualCanvas";
import {
  VISUAL_COLORS,
  VISUAL_COLOR_LABELS,
  VISUAL_STYLES,
  VISUAL_STYLE_LABELS,
  coverColor,
  readVisualColor,
  readVisualStyle,
  showsWholeTrack,
  writeVisualColor,
  writeVisualStyle,
  type VisualColor,
  type VisualStyle,
} from "./audioVisual";
import { LibraryArtwork } from "./LibraryArtwork";
import { installationTitle } from "./libraryPresentation";
import { MediaDockMenu, MediaDockMenuGroup, MediaDockMenuItem } from "./MediaDockMenu";
import { useMediaPlayback } from "./MediaPlaybackProvider";
import { mediaItemName, orderedSessionItems } from "./mediaSession";
import { useOwnWork } from "./ownWorkDetails";
import { TrackWaveform, useWarmWaveforms } from "./TrackWaveform";
import type { Installation, LibraryGateway, MediaSession } from "./types";

const COVER_SAMPLE_PX = 24;
const RAINBOW_SWATCH =
  "linear-gradient(90deg, hsl(0 85% 64%), hsl(100 85% 64%), hsl(200 85% 64%), hsl(300 85% 64%))";

/**
 * The colour the stage's artwork gives the "From the cover" set. The cover
 * is loaded once more as a picture the page may read; one it may not read
 * gives no colour.
 */
function useCoverColor(stageRef: RefObject<HTMLElement | null>, artwork: string): string | null {
  const [color, setColor] = useState<string | null>(null);
  useEffect(() => {
    const image = stageRef.current?.querySelector<HTMLImageElement>(".library-artwork img");
    if (!image) {
      setColor(null);
      return undefined;
    }
    let stale = false;
    const read = () => {
      if (!image.currentSrc) return;
      const copy = new Image();
      copy.crossOrigin = "anonymous";
      copy.onload = () => {
        if (stale) return;
        try {
          const canvas = document.createElement("canvas");
          canvas.width = COVER_SAMPLE_PX;
          canvas.height = COVER_SAMPLE_PX;
          const context = canvas.getContext("2d");
          if (!context) return;
          context.drawImage(copy, 0, 0, COVER_SAMPLE_PX, COVER_SAMPLE_PX);
          setColor(coverColor(context.getImageData(0, 0, COVER_SAMPLE_PX, COVER_SAMPLE_PX).data));
        } catch {
          setColor(null);
        }
      };
      copy.src = image.currentSrc;
    };
    if (image.complete) read();
    image.addEventListener("load", read);
    return () => {
      stale = true;
      image.removeEventListener("load", read);
    };
  }, [artwork, stageRef]);
  return color;
}

interface AudioPlayerProps {
  gateway: LibraryGateway;
  installation: Installation;
  session: MediaSession;
  work: CatalogWork | null;
  onBack: () => void | Promise<void>;
}

/**
 * The full-screen audio player: the work's artwork with the waveform or the
 * analyser over it, and the track list. Playback runs in the app's player,
 * whose dock is the transport; leaving closes the player, as the video player
 * does, and closing it from the dock leaves the page.
 */
export function AudioPlayer({ gateway, installation, session, work, onBack }: AudioPlayerProps) {
  const { t } = usePresentation();
  const playback = useMediaPlayback();
  const own = useOwnWork();
  const current = playback.session?.installationId === installation.id;
  const [visualStyle, setVisualStyle] = useState<VisualStyle>(readVisualStyle);
  const [visualColor, setVisualColor] = useState<VisualColor>(readVisualColor);
  const [visualMenuOpen, setVisualMenuOpen] = useState(false);
  const stageRef = useRef<HTMLDivElement>(null);
  const [opened, setOpened] = useState(false);
  const [openFailed, setOpenFailed] = useState(false);
  const rootRef = useRef<HTMLElement>(null);
  const onBackRef = useRef(onBack);
  onBackRef.current = onBack;
  const requestedRef = useRef(false);
  const openWork = playback.openWork;

  useEffect(() => {
    if (current) {
      setOpened(true);
      return;
    }
    if (opened) {
      void onBackRef.current();
      return;
    }
    if (requestedRef.current) return;
    requestedRef.current = true;
    void openWork(installation.id).then((result) => {
      if (!result) setOpenFailed(true);
    });
  }, [current, installation.id, opened, openWork]);

  const volumeRef = useRef(playback.volume);
  volumeRef.current = playback.volume;
  const setVolume = playback.setVolume;
  // The wheel anywhere but over the track list turns the volume, as over the
  // video picture. A listener of its own, since React's wheel handlers cannot
  // stop the page from scrolling.
  useEffect(() => {
    const root = rootRef.current;
    if (!root) return;
    const onWheel = (event: WheelEvent) => {
      if (event.deltaY === 0 || isInteractiveTarget(event.target)) return;
      if (event.target instanceof Element && event.target.closest(".audio-player-queue")) return;
      event.preventDefault();
      setVolume(volumeRef.current + (event.deltaY < 0 ? VOLUME_WHEEL_STEP : -VOLUME_WHEEL_STEP));
    };
    root.addEventListener("wheel", onWheel, { passive: false });
    return () => root.removeEventListener("wheel", onWheel);
  }, [setVolume]);

  const items = current ? playback.items : orderedSessionItems(session, session.shuffle);
  const item = current
    ? playback.item
    : items.find((candidate) => candidate.ordinal === session.progress.itemOrdinal) ?? items[0] ?? null;
  const index = item ? items.findIndex((candidate) => candidate.ordinal === item.ordinal) : -1;
  useWarmWaveforms(
    gateway,
    installation.id,
    current && index >= 0
      ? [...items.slice(index), ...items.slice(0, index)].map((entry) => entry.ordinal)
      : [],
  );
  const title = installationTitle(installation);
  const ownCover = own.coverUrl(installation.id);
  const cover = useCoverColor(stageRef, `${installation.id}|${work?.code ?? ""}|${ownCover ?? ""}`);
  const wholeTrack = showsWholeTrack(visualStyle);
  const failure = openFailed ? t("media.openFailed") : playback.error;

  return (
    <main ref={rootRef} className="audio-player">
      <header className="audio-player-topbar">
        <BackButton onClick={playback.close} />
        <div className="audio-player-title">
          <span><Headphones aria-hidden="true" />{t("media.player.audio")}</span>
          <strong title={title}>{title}</strong>
          <small>{item ? mediaItemName(item) : t("media.noItems")}</small>
        </div>
        <div className="audio-player-position" aria-live="polite">
          <strong>{items.length > 0 ? index + 1 : 0}</strong><span>/ {items.length}</span>
        </div>
      </header>

      <div className="audio-player-workspace">
        <div ref={stageRef} className="audio-player-stage">
          <LibraryArtwork kind="audio" title={title} work={work ?? undefined} cover={ownCover} />
          <span className="audio-player-shade" aria-hidden="true" />
          <div className="audio-player-visual-menu">
            <MediaDockMenu
              label={t("media.visual.menu")}
              icon={<AudioLines aria-hidden="true" />}
              open={visualMenuOpen}
              side="bottom"
              onOpenChange={setVisualMenuOpen}
            >
              <MediaDockMenuGroup labelKey="media.visual.style">
                {VISUAL_STYLES.map((style) => (
                  <MediaDockMenuItem
                    key={style}
                    active={style === visualStyle}
                    label={t(VISUAL_STYLE_LABELS[style])}
                    keepOpen
                    onSelect={() => {
                      setVisualStyle(style);
                      writeVisualStyle(style);
                    }}
                  />
                ))}
              </MediaDockMenuGroup>
              <MediaDockMenuGroup labelKey="media.visual.color">
                {VISUAL_COLORS.map((color) => (
                  <MediaDockMenuItem
                    key={color}
                    active={color === visualColor}
                    label={t(VISUAL_COLOR_LABELS[color])}
                    swatch={{
                      theme: "var(--color-primary)",
                      cover: cover ?? "var(--color-primary)",
                      white: "#ffffff",
                      ice: "#6fe3ff",
                      rainbow: RAINBOW_SWATCH,
                    }[color]}
                    keepOpen
                    onSelect={() => {
                      setVisualColor(color);
                      writeVisualColor(color);
                    }}
                  />
                ))}
              </MediaDockMenuGroup>
            </MediaDockMenu>
          </div>
          <div className={`audio-player-visualizer-stage${visualStyle === "ring" ? " is-tall" : ""}`}>
            {item ? (
              <div
                className={`audio-player-visualizer-layer${wholeTrack ? " is-active" : ""}`}
                aria-hidden={!wholeTrack}
                inert={!wholeTrack}
              >
                <TrackWaveform
                  gateway={gateway}
                  installationId={installation.id}
                  ordinal={item.ordinal}
                  resume={null}
                  visualStyle={visualStyle}
                  visualColor={visualColor}
                  coverColor={cover}
                  onStart={async (ordinal, positionSeconds) => {
                    if (current) playback.selectOrdinal(ordinal, positionSeconds);
                  }}
                />
              </div>
            ) : null}
            <div
              className={`audio-player-visualizer-layer is-live${wholeTrack ? "" : " is-active"}`}
              aria-hidden="true"
            >
              <LiveVisual
                installationId={installation.id}
                style={visualStyle}
                color={visualColor}
                cover={cover}
                active={!wholeTrack}
              />
            </div>
          </div>
          {failure ? <div className="app-callout audio-player-error" role="alert">{failure}</div> : null}
        </div>

        <aside className="audio-player-queue" aria-label={t("media.queue")}>
          <header>
            <span>{t("media.queue")}</span>
            <strong>{items.length}</strong>
          </header>
          <ol>
            {items.map((entry, position) => {
              const active = item?.ordinal === entry.ordinal;
              return (
                <li key={entry.ordinal}>
                  <button
                    className={active ? "is-active" : undefined}
                    type="button"
                    aria-current={active ? "true" : undefined}
                    disabled={!current}
                    onClick={() => playback.selectOrdinal(entry.ordinal)}
                  >
                    <span>{String(position + 1).padStart(2, "0")}</span>
                    <strong title={mediaItemName(entry)}>{mediaItemName(entry)}</strong>
                    {active && playback.playing
                      ? <i aria-label={t("media.nowPlaying")}><Pause aria-hidden="true" /></i>
                      : null}
                  </button>
                </li>
              );
            })}
          </ol>
        </aside>
      </div>
    </main>
  );
}
