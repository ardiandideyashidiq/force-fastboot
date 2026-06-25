export type Theme = "light" | "dark";

export interface TauriEvent<T> {
  payload: T;
}

export interface ProgressEvent {
  phase: "idle" | "waiting" | "running" | "complete" | "cancelled" | "error";
  percent: number;
  message: string;
}

export type TauriResult<T> =
  | { status: "ok"; data: T }
  | { status: "error"; message: string };

export interface DeviceInfo {
  connected: boolean;
  serial: string | null;
  vars: Record<string, string>;
}

export interface FlashAction {
  action: string;
  partition: string;
  base_name: string;
  slot: string | null;
  layout: string;
  region: string;
  start: number;
  size: number;
  size_human: string;
  image: { path: { resolved_path: string; exists: boolean } } | null;
  safety_class: string;
  reason: string;
  warnings: string[];
}

export interface FlashPlan {
  mode: string;
  storage_selection: string;
  selected_layouts: string[];
  platform: string | null;
  project: string | null;
  firmware_dir: string | null;
  package_root: string | null;
  summary: {
    flash_count: number;
    skipped_count: number;
    missing_image_count: number;
    warning_count: number;
    error_count: number;
  };
  actions: FlashAction[];
  skipped: Array<{ partition: string; reason: string }>;
  warnings: string[];
  errors: string[];
}

export interface ScatterFile {
  path: string;
  format: string;
  platform: string | null;
  project: string | null;
  chipset: string | null;
  layout_names: string[];
  partition_count: number;
  warnings: string[];
  errors: string[];
}
