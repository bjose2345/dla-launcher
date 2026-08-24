export interface SystemReport {
  os: string;
  osVersion: string;
  kernel: string;
  arch: string;
  cpu: string;
  cpuCores: number;
  memoryBytes: number;
  webview: string;
}

export interface SystemGateway {
  readSystemReport(): Promise<SystemReport>;
}

export interface Developer {
  id: string;
  name: string;
  quote: string;
  quoteEmoji?: string;
  effect: "aura" | "arcane";
  portrait: string;
}
