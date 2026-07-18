export type Theme = "light" | "dark";

export interface ConfirmAction {
  title: string;
  description: string;
  confirmLabel?: string;
  variant?: "destructive" | "default";
  onConfirm: () => void;
}

export interface DeviceInfo {
  connected: boolean;
  serial: string | null;
  vars: Record<string, string>;
}

export interface ScatterFile {
  path: string;
  format: string;
  text_hash: string;
  platform: string | null;
  project: string | null;
  general: unknown;
  layouts: Record<string, unknown[]>;
  warnings: string[];
  errors: string[];
}
