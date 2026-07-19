export type ProgressEvent =
  | { event: "Phase"; data: { phase: string; message: string } }
  | { event: "FlashProgress"; data: { partition: string; percent: number } }
  | { event: "FlashComplete"; data: { partition: string; success: boolean; response: string | null } }
  | { event: "DeviceAction"; data: { action: string; detail: string } }
  | { event: "Overall"; data: { current: number; total: number } }
  | { event: "Warning"; data: { message: string } }
  | { event: "Error"; data: { message: string } }
  | { event: "Done"; data: { ok: boolean; detail: string } };

export interface ConsoleEntry {
  id: number;
  timestamp: number;
  text: string;
  type: "command" | "response" | "success" | "error" | "warning" | "info";
}
