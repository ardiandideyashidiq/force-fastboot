import { useState, useEffect, useRef, type ReactNode } from "react";
import { Button } from "@/components/ui/button";
import {
  Zap,
  Wrench,
  Settings,
  PanelLeftClose,
  PanelLeftOpen,
  Sun,
  Moon,
} from "lucide-react";

const SIDEBAR_OPEN = 224;  // 14rem
const SIDEBAR_COLLAPSED = 60; // 3.75rem

interface AppLayoutProps {
  children: (props: { tab: string }) => ReactNode;
  sidebarActions?:
    | ReactNode
    | ((props: { sidebarOpen: boolean }) => ReactNode);
  theme: "light" | "dark";
  onThemeChange: (
    theme: "light" | "dark" | ((current: "light" | "dark") => "light" | "dark"),
  ) => void;
}

const navItems = [
  { id: "main", label: "Flasher", icon: Zap },
  { id: "tools", label: "Tools", icon: Wrench },
  { id: "settings", label: "Settings", icon: Settings },
];

export default function AppLayout({
  children,
  sidebarActions,
  theme,
  onThemeChange,
}: AppLayoutProps) {
  const [sidebarOpen, setSidebarOpen] = useState(true);
  const [tab, setTab] = useState("main");
  const userOverride = useRef(false);
  const mainRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const mq = window.matchMedia("(max-width: 1100px)");
    const handler = (e: MediaQueryListEvent | MediaQueryList) => {
      if (!userOverride.current) {
        setSidebarOpen(!e.matches);
      } else if (!e.matches) {
        userOverride.current = false;
        setSidebarOpen(true);
      }
    };
    handler(mq);
    mq.addEventListener("change", handler);
    return () => mq.removeEventListener("change", handler);
  }, []);

  const toggleSidebar = () => {
    userOverride.current = true;
    setSidebarOpen((prev) => !prev);
  };

  useEffect(() => {
    mainRef.current?.scrollTo(0, 0);
  }, [tab]);

  useEffect(() => {
    document.documentElement.classList.toggle("dark", theme === "dark");
    localStorage.setItem("app-theme", theme);
  }, [theme]);

  const toggleTheme = () => {
    onThemeChange((current) => (current === "dark" ? "light" : "dark"));
  };

  return (
    <div
      className="grid h-dvh w-dvw overflow-hidden"
      style={{ gridTemplateColumns: "auto 1fr" }}
    >
      {/* Sidebar */}
      <aside
        className="flex flex-col border-r border-sidebar-border bg-sidebar overflow-hidden min-w-0 transition-[width] duration-300 ease-out"
        style={{ width: sidebarOpen ? SIDEBAR_OPEN : SIDEBAR_COLLAPSED }}
      >
        {/* Brand + collapse */}
        <div className="flex items-center justify-between shrink-0 px-4 pt-4 pb-3 border-b border-accent-brand/15">
          {sidebarOpen ? (
            <span className="text-caption font-semibold tracking-overline text-muted-foreground/70 uppercase">
              pawflash
            </span>
          ) : null}
          <Button
            variant="ghost"
            size="icon-sm"
            onClick={toggleSidebar}
            aria-label={sidebarOpen ? "Collapse sidebar" : "Expand sidebar"}
          >
            {sidebarOpen ? <PanelLeftClose size={18} /> : <PanelLeftOpen size={18} />}
          </Button>
        </div>

        {/* Nav */}
        <nav className="flex flex-col gap-1 px-3 shrink-0">
          {navItems.map((item) => {
            const Icon = item.icon;
            const isActive = tab === item.id;
            return (
              <Button
                key={item.id}
                variant="ghost"
                size={sidebarOpen ? "default" : "icon-sm"}
                className={
                  "relative after:absolute after:left-0 after:top-1/2 after:-translate-y-1/2 after:h-4 after:w-0.5 after:rounded-full after:transition-opacity after:duration-200 " +
                  (isActive
                    ? "bg-accent-brand/10 text-accent-brand after:bg-accent-brand after:opacity-100"
                    : "text-muted-foreground hover:text-foreground hover:bg-muted/40 after:opacity-0")
                }
                onClick={() => setTab(item.id)}
              >
                <Icon size={18} />
                {sidebarOpen && <span>{item.label}</span>}
              </Button>
            );
          })}
        </nav>

        {/* Spacer */}
        <div className="min-h-0 flex-1" />

        {/* Actions slot */}
        {sidebarActions && (
          <div className="mb-3 px-4">
            {typeof sidebarActions === "function"
              ? sidebarActions({ sidebarOpen })
              : sidebarActions}
          </div>
        )}

        {/* Theme toggle */}
        <div className="shrink-0 border-t border-sidebar-border px-4 py-4">
          {sidebarOpen ? (
            <div className="grid grid-cols-2 gap-1.5">
              <Button
                variant={theme === "light" ? "secondary" : "ghost"}
                size="sm"
                onClick={() => onThemeChange("light")}
                className="w-full"
              >
                <Sun size={16} />
                <span>Light</span>
              </Button>
              <Button
                variant={theme === "dark" ? "secondary" : "ghost"}
                size="sm"
                onClick={() => onThemeChange("dark")}
                className="w-full"
              >
                <Moon size={16} />
                <span>Dark</span>
              </Button>
            </div>
          ) : (
            <Button
              variant="ghost"
              size="icon-sm"
              onClick={toggleTheme}
              aria-label={`Switch to ${theme === "dark" ? "light" : "dark"} mode`}
              className="w-full"
            >
              {theme === "dark" ? <Sun size={18} /> : <Moon size={18} />}
            </Button>
          )}
        </div>
      </aside>

      {/* Main */}
      <main ref={mainRef} className="overflow-y-auto p-5 max-sm:p-3">
        {children({ tab })}
      </main>
    </div>
  );
}
