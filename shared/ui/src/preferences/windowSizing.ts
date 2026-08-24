export type WindowSize = {
  width: number;
  height: number;
};

export interface WindowMetrics extends WindowSize {
  workAreaWidth: number;
  workAreaHeight: number;
  scaleFactor: number;
  maximized: boolean;
  supportsWindowControls: boolean;
}

export interface WindowGateway {
  readWindowMetrics(): Promise<WindowMetrics>;
  resizeWindow(size: WindowSize): Promise<WindowMetrics>;
  maximizeWindow(): Promise<WindowMetrics>;
}

export interface WindowPreset extends WindowSize {
  id: string;
  label: string;
}

export const windowPresets: ReadonlyArray<WindowPreset> = [
  { id: "hd", label: "1280 × 720", width: 1280, height: 720 },
  { id: "laptop", label: "1366 × 768", width: 1366, height: 768 },
  { id: "desktop", label: "1440 × 900", width: 1440, height: 900 },
  { id: "full-hd", label: "1920 × 1080", width: 1920, height: 1080 },
  { id: "2k", label: "2560 × 1440", width: 2560, height: 1440 },
  { id: "4k", label: "3840 × 2160", width: 3840, height: 2160 },
];

export function clampWindowSize(requested: WindowSize, workArea: WindowSize): WindowSize {
  return {
    width: Math.min(Math.max(requested.width, 360), workArea.width),
    height: Math.min(Math.max(requested.height, 560), workArea.height),
  };
}
