import { useState, useEffect, useRef, type ReactNode } from "react";
import { Button } from "@/components/ui/button";
import { Separator } from "@/components/ui/separator";
import {
  Zap,
  Settings2,
  Layers3,
  PanelLeftClose,
  PanelLeftOpen,
  Sun,
  Moon,
} from "lucide-react";

interface AppLayoutProps {
  children: (props: { tab: string }) => ReactNode;
  sidebarStatus?: ReactNode;
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
  { id: "tools", label: "Tools", icon: Settings2 },
  { id: "settings", label: "Settings", icon: Layers3 },
];

export default function AppLayout({
  children,
  sidebarStatus,
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
      className="grid h-dvh w-dvw overflow-hidden transition-[grid-template-columns] duration-200 ease-out"
      style={{
        gridTemplateColumns: sidebarOpen ? "14rem 1fr" : "4.5rem 1fr",
      }}
    >
      {/* Sidebar */}
      <aside className="flex flex-col border-r border-border bg-sidebar px-3 py-4 overflow-hidden">
        {/* Brand + collapse */}
        <div className="flex items-center justify-between mb-4">
          {sidebarOpen && (
            <span className="text-sm font-semibold tracking-[0.16em] text-muted-foreground">
              PAWFLASH
            </span>
          )}
          <Button
            variant="ghost"
            size="icon-sm"
            onClick={toggleSidebar}
            aria-label={sidebarOpen ? "Collapse sidebar" : "Expand sidebar"}
          >
            {sidebarOpen ? <PanelLeftClose size={18} /> : <PanelLeftOpen size={18} />}
          </Button>
        </div>

        <Separator />

        {/* Nav */}
        <nav className="flex flex-col gap-1 mt-3">
          {navItems.map((item) => {
            const Icon = item.icon;
            const isActive = tab === item.id;
            return (
              <Button
                key={item.id}
                variant="ghost"
                size={sidebarOpen ? "default" : "icon-sm"}
                className={
                  isActive
                    ? "bg-accent-brand/12 text-accent-soft-foreground shadow-[var(--panel-shadow)] border border-accent-brand/25"
                    : "text-muted-foreground hover:bg-accent-soft/70 hover:text-foreground"
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

        {/* Status slot */}
        {sidebarStatus && (
          <div className="mb-3">
            {sidebarStatus}
          </div>
        )}

        {/* Actions slot */}
        {sidebarActions && (
          <div className="mb-3">
            {typeof sidebarActions === "function"
              ? sidebarActions({ sidebarOpen })
              : sidebarActions}
          </div>
        )}

        <Separator />

        {/* Theme toggle */}
        <div className="mt-3">
          {sidebarOpen ? (
            <div className="grid grid-cols-2 gap-2">
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
      <main ref={mainRef} className="overflow-y-auto p-6">
        {children({ tab })}
      </main>
    </div>
  );
}
