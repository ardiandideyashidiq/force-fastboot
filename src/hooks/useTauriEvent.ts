import { useEffect, useRef } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export function useTauriEvent<T = unknown>(
  eventName: string,
  handler: (payload: T) => void,
) {
  const handlerRef = useRef(handler);

  useEffect(() => {
    handlerRef.current = handler;
  });

  useEffect(() => {
    let cancelled = false;
    let unlisten: UnlistenFn | undefined;

    listen<T>(eventName, (event) => {
      if (!cancelled) handlerRef.current(event.payload);
    }).then((fn) => {
      if (cancelled) {
        fn();
      } else {
        unlisten = fn;
      }
    });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [eventName]);
}
