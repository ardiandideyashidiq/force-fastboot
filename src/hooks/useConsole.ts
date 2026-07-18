import { useContext } from "react";
import { ConsoleContext, type ConsoleContextType } from "@/components/console/ConsoleContext";

export function useConsole(): ConsoleContextType {
  const ctx = useContext(ConsoleContext);
  if (!ctx) throw new Error("useConsole must be used within ConsoleProvider");
  return ctx;
}
