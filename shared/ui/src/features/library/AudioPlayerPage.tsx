import { AudioLines, AudioWaveform, Headphones, Pause } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import { BackButton } from "../../app/BackButton";
import { readStoredText, writeStoredText } from "../../preferences/browserStorage";
import { isInteractiveTarget } from "../../preferences/keyBindings";
import { usePresentation } from "../../preferences/PresentationProvider";
import type { CatalogWork } from "../catalog";
import { VOLUME_WHEEL_STEP } from "./audioPlayer";
import { LibraryArtwork } from "./LibraryArtwork";
import { installationTitle } from "./libraryPresentation";
import { useMediaPlayback } from "./MediaPlaybackProvider";
import { mediaItemName, orderedSessionItems } from "./mediaSession";
import { NowPlayingBars } from "./NowPlayingBars";
import { useOwnWork } from "./ownWorkDetails";
import { TrackWaveform } from "./TrackWaveform";
import type { Installation, LibraryGateway, MediaSession } from "./types";

const VISUALIZER_STORAGE_KEY = "dla.library.audio-visualizer";

type VisualizerMode = "waveform" | "analyser";

function readVisualizerMode(): VisualizerMode {
  return readStoredText(VISUALIZER_STORAGE_KEY) === "analyser" ? "analyser" : "waveform";
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
  const [visualizerMode, setVisualizerMode] = useState<VisualizerMode>(readVisualizerMode);
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
  const title = installationTitle(installation);
  const chooseVisualizer = (mode: VisualizerMode) => {
    setVisualizerMode(mode);
    writeStoredText(VISUALIZER_STORAGE_KEY, mode);
  };
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
        <div className="audio-player-stage">
          <LibraryArtwork kind="audio" title={title} work={work ?? undefined} cover={own.coverUrl(installation.id)} />
          <span className="audio-player-shade" aria-hidden="true" />
          <div className="audio-player-visualizer-toggle">
            <button
              className={visualizerMode === "waveform" ? "is-active" : undefined}
              type="button"
              aria-pressed={visualizerMode === "waveform"}
              onClick={() => chooseVisualizer("waveform")}
            >
              <AudioWaveform aria-hidden="true" />
              {t("media.visualizer.waveform")}
            </button>
            <button
              className={visualizerMode === "analyser" ? "is-active" : undefined}
              type="button"
              aria-pressed={visualizerMode === "analyser"}
              onClick={() => chooseVisualizer("analyser")}
            >
              <AudioLines aria-hidden="true" />
              {t("media.visualizer.analyser")}
            </button>
          </div>
          <div className="audio-player-visualizer-stage">
            {item ? (
              <div
                className={`audio-player-visualizer-layer${visualizerMode === "waveform" ? " is-active" : ""}`}
                aria-hidden={visualizerMode !== "waveform"}
                inert={visualizerMode !== "waveform"}
              >
                <TrackWaveform
                  gateway={gateway}
                  installationId={installation.id}
                  ordinal={item.ordinal}
                  resume={null}
                  onStart={async (ordinal, positionSeconds) => {
                    if (current) playback.selectOrdinal(ordinal, positionSeconds);
                  }}
                />
              </div>
            ) : null}
            <div
              className={`audio-player-visualizer-layer is-analyser${visualizerMode === "analyser" ? " is-active" : ""}`}
              aria-hidden={visualizerMode !== "analyser"}
            >
              <NowPlayingBars
                installationId={installation.id}
                alwaysVisible
                liveSpectrum={visualizerMode === "analyser"}
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
