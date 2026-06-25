import { useEffect, useRef, useCallback } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { invoke as tauriInvoke } from "@tauri-apps/api/core";

export function useTauriEvent<T = unknown>(
  eventName: string,
  handler: (payload: T) => void,
) {
  const handlerRef = useRef(handler);
  handlerRef.current = handler;

  useEffect(() => {
    let unlisten: UnlistenFn;
    let cancelled = false;

    listen<T>(eventName, (event) => {
      if (!cancelled) handlerRef.current(event.payload);
    }).then((fn) => {
      unlisten = fn;
    });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [eventName]);
}

export function useTauriInvoke() {
  const call = useCallback(async <T>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
    return tauriInvoke(cmd, args);
  }, []);

  return call;
}
