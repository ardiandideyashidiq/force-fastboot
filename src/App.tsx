import { useState, useEffect, lazy, Suspense, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { toast, Toaster } from "sonner";
import AppLayout from "@/components/layout/AppLayout";
import { ConsoleProvider } from "@/components/console/ConsoleContext";
import { Button } from "@/components/ui/button";
import { Select, SelectTrigger, SelectContent, SelectItem } from "@/components/ui/select";
import { RotateCcw, RefreshCw } from "lucide-react";
import type { Theme, DeviceInfo } from "@/types/api";

const MainTab = lazy(() => import("@/components/tabs/MainTab"));
const ToolsTab = lazy(() => import("@/components/tabs/ToolsTab"));
const SettingsTab = lazy(() => import("@/components/tabs/SettingsTab"));

const rebootTargets = [
  { value: "system", label: "System" },
  { value: "bootloader", label: "Bootloader" },
  { value: "fastbootd", label: "Fastbootd" },
  { value: "recovery", label: "Recovery" },
];

function App() {
  const [theme, setTheme] = useState<Theme>(() => {
    const root = document.documentElement;
    return root.classList.contains("dark") ? "dark" : "light";
  });
  const [rebooting, setRebooting] = useState<string | null>(null);
  const [device, setDevice] = useState<DeviceInfo | null>(null);
  const [deviceLoading, setDeviceLoading] = useState(false);

  const fetchDevice = useCallback(async () => {
    setDeviceLoading(true);
    try {
      const info = await invoke<DeviceInfo>("get_device_info");
      setDevice(info);
    } catch (e) {
      toast.error(`Failed to get device info: ${e}`);
    }
    setDeviceLoading(false);
  }, []);

  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect
    fetchDevice();
  }, [fetchDevice]);

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "r" && !e.metaKey && !e.ctrlKey && !(e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) && !(e.target instanceof HTMLElement && e.target.contentEditable === "true")) {
        fetchDevice();
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [fetchDevice]);

  const handleReboot = useCallback(async (target: string | null) => {
    if (!target) return;
    setRebooting(target);
    try {
      await invoke("reboot_device", { target });
    } catch (e) {
      toast.error(`Reboot failed: ${e}`);
    }
    setRebooting(null);
  }, []);

  const connected = device?.connected ?? false;
  const vars = device?.vars ?? {};

  return (
    <ConsoleProvider>
      <AppLayout
        theme={theme}
        onThemeChange={setTheme}
        sidebarActions={({ sidebarOpen }) => (
          <div className="space-y-3">
            {/* Reboot dropdown */}
            <Select value="" onValueChange={handleReboot} disabled={!!rebooting}>
              <SelectTrigger
                className={sidebarOpen ? "w-full justify-start gap-2" : "w-full justify-center px-0"}
                size="sm"
              >
                <RotateCcw size={18} className={rebooting ? "animate-spin" : ""} />
                {sidebarOpen && "Reboot"}
              </SelectTrigger>
              <SelectContent side="right" align="start" sideOffset={8} collisionAvoidance={{ side: 'none' }} alignItemWithTrigger={false}>
                {rebootTargets.map((t) => (
                  <SelectItem key={t.value} value={t.value}>
                    {t.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>

            {/* Device status */}
            {sidebarOpen ? (
              <div className="rounded-sm border border-border/60 bg-muted/30 px-2.5 py-2 text-xs space-y-1.5">
                <div className="flex items-center gap-2">
                  <span className={`size-2 rounded-full transition-colors duration-300 ${connected ? "dot-complete" : "dot-waiting animate-pulse"}`} />
                  <span className={connected ? "text-foreground font-medium" : "text-muted-foreground"}>
                    {connected ? "Connected" : "Disconnected"}
                  </span>
                  <Button variant="ghost" size="icon-xs" className="ml-auto" onClick={fetchDevice} disabled={deviceLoading}>
                    <RefreshCw size={14} className={deviceLoading ? "animate-spin" : ""} />
                  </Button>
                </div>
                {connected && (
                  <div className="text-muted-foreground space-y-0.5">
                    <div className="text-trace-copper font-mono text-caption">
                      {device?.serial ?? "—"}
                    </div>
                    <div className="flex items-center gap-1.5 text-caption">
                      <span className="text-muted-foreground">{vars.product ?? "—"}</span>
                      <span className="text-muted-foreground/40">·</span>
                      <span className="text-muted-foreground">
                        slot {vars["current-slot"] ?? "—"}
                      </span>
                    </div>
                  </div>
                )}
              </div>
            ) : (
              <Button
                variant="ghost"
                size="icon-sm"
                className="w-full justify-center px-0"
                onClick={fetchDevice}
                disabled={deviceLoading}
              >
                <span className={`size-2 rounded-full transition-colors duration-300 ${connected ? "dot-complete" : "dot-waiting animate-pulse"}`} />
              </Button>
            )}
          </div>
        )}
      >
        {({ tab }) => (
          <Suspense fallback={null}>
            <div key={tab} className="animate-in fade-in duration-200 ease-out">
              {tab === "main" && <MainTab device={device} onRefresh={fetchDevice} />}
              {tab === "tools" && <ToolsTab />}
              {tab === "settings" && <SettingsTab />}
            </div>
          </Suspense>
        )}
      </AppLayout>
      <Toaster richColors position="top-center" theme={theme} />
    </ConsoleProvider>
  );
}

export default App;
