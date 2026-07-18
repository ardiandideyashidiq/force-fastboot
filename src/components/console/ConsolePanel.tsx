import { useRef, useEffect, useState } from "react";
import { useConsole } from "@/hooks/useConsole";
import { Button } from "@/components/ui/button";
import { Terminal, Trash2, ChevronDown, ChevronRight } from "lucide-react";

function formatTime(ms: number): string {
  const s = (ms / 1000).toFixed(2);
  return `+${s.padStart(7, "0")}s`;
}

function entryIcon(type: string): string {
  switch (type) {
    case "success":
      return "✓";
    case "error":
      return "✗";
    case "warning":
      return "⚠";
    case "command":
      return "$";
    case "response":
      return "⇾";
    default:
      return "·";
  }
}

function entryColor(type: string): string {
  switch (type) {
    case "success":
      return "text-signal-green";
    case "error":
      return "text-signal-red";
    case "warning":
      return "text-signal-amber";
    case "command":
      return "text-trace-copper";
    case "response":
      return "text-foreground/70";
    default:
      return "text-muted-foreground";
  }
}

const MAX_HEIGHT = 240;

export default function ConsolePanel() {
  const { entries, clearConsole } = useConsole();
  const [collapsed, setCollapsed] = useState(false);
  const scrollRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [entries]);

  return (
    <div className="border-t border-border bg-[var(--card)]">
      {/* Header */}
      <div className="flex items-center justify-between px-4 py-1.5">
        <div className="flex items-center gap-2">
          <Terminal size={14} className="text-trace-copper" />
          <span className="text-caption font-display font-medium uppercase tracking-wider text-trace-copper">
            Console
          </span>
          <span className="text-caption text-muted-foreground tabular-nums">
            {entries.length}
          </span>
        </div>
        <div className="flex items-center gap-0.5">
          {entries.length > 0 && (
            <Button
              variant="ghost"
              size="icon-xs"
              onClick={clearConsole}
              aria-label="Clear console"
            >
              <Trash2 size={12} />
            </Button>
          )}
          <Button
            variant="ghost"
            size="icon-xs"
            onClick={() => setCollapsed((c) => !c)}
            aria-label={collapsed ? "Expand console" : "Collapse console"}
          >
            {collapsed ? <ChevronRight size={14} /> : <ChevronDown size={14} />}
          </Button>
        </div>
      </div>

      {/* Entry list */}
      {!collapsed && (
        <div
          ref={scrollRef}
          className="overflow-y-auto overscroll-contain border-t border-border/50"
          style={{ maxHeight: MAX_HEIGHT }}
        >
          {entries.length === 0 ? (
            <p className="text-caption text-muted-foreground/40 py-3 px-4 font-mono">
              · waiting for output
            </p>
          ) : (
            <div className="px-4 py-1.5 space-y-0.5">
              {entries.map((entry) => (
                <div
                  key={entry.id}
                  className="flex items-start gap-2 text-caption font-mono leading-snug"
                >
                  <span className="text-muted-foreground/40 shrink-0 w-[4.5rem] tabular-nums">
                    {formatTime(entry.timestamp)}
                  </span>
                  <span className={`shrink-0 w-3 text-center ${entryColor(entry.type)}`}>
                    {entryIcon(entry.type)}
                  </span>
                  <span className={entryColor(entry.type)}>{entry.text}</span>
                </div>
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
