import { createContext, useState, useCallback, useRef, type ReactNode } from "react";
import type { ProgressEvent, ConsoleEntry } from "@/types/progress";

export interface ConsoleContextType {
  entries: ConsoleEntry[];
  addEntry(entry: Omit<ConsoleEntry, "id" | "timestamp">): void;
  addProgressEvent(event: ProgressEvent): void;
  clearConsole(): void;
}

// eslint-disable-next-line react-refresh/only-export-components
export const ConsoleContext = createContext<ConsoleContextType | null>(null);

export function ConsoleProvider({ children }: { children: ReactNode }) {
  const [entries, setEntries] = useState<ConsoleEntry[]>([]);
  const [startTime] = useState(() => Date.now());
  const nextId = useRef(0);

  const addEntry = useCallback(
    (entry: Omit<ConsoleEntry, "id" | "timestamp">) => {
      const id = nextId.current;
      nextId.current += 1;
      setEntries((prev) => [
        ...prev,
        { ...entry, id, timestamp: Date.now() - startTime },
      ]);
    },
    [startTime],
  );

  const addProgressEvent = useCallback(
    (event: ProgressEvent) => {
      switch (event.event) {
        case "Phase":
          addEntry({ text: event.data.message, type: "info" });
          break;
        case "FlashProgress":
          addEntry({
            text: `[${event.data.partition}] ${Math.round(event.data.percent)}%`,
            type: "info",
          });
          break;
        case "FlashComplete": {
          const label = event.data.success ? "OK" : "FAIL";
          const resp = event.data.response ? ` — ${event.data.response}` : "";
          addEntry({
            text: `${event.data.partition}: ${label}${resp}`,
            type: event.data.success ? "success" : "error",
          });
          break;
        }
        case "FormatProgress":
          addEntry({
            text: `[${event.data.partition}] ${event.data.status}`,
            type: "info",
          });
          break;
        case "DeviceAction":
          addEntry({
            text: `${event.data.action}: ${event.data.detail}`,
            type: "info",
          });
          break;
        case "Overall":
          addEntry({
            text: `Progress ${event.data.current}/${event.data.total}`,
            type: "info",
          });
          break;
        case "Warning":
          addEntry({ text: event.data.message, type: "warning" });
          break;
        case "Error":
          addEntry({ text: event.data.message, type: "error" });
          break;
        case "Done":
          addEntry({
            text: event.data.detail,
            type: event.data.ok ? "success" : "error",
          });
          break;
      }
    },
    [addEntry],
  );

  const clearConsole = useCallback(() => {
    setEntries([]);
    nextId.current = 0;
  }, []);

  return (
    <ConsoleContext.Provider value={{ entries, addEntry, addProgressEvent, clearConsole }}>
      {children}
    </ConsoleContext.Provider>
  );
}
