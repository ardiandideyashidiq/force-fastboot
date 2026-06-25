import { useState, useEffect, lazy, Suspense, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { toast, Toaster } from "sonner";
import AppLayout from "@/components/layout/AppLayout";
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

  const onThemeChange = useCallback(
    (value: Theme | ((current: Theme) => Theme)) => {
      setTheme(value);
    },
    [],
  );

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
    fetchDevice();
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
    <>
      <AppLayout
        theme={theme}
        onThemeChange={onThemeChange}
        sidebarActions={({ sidebarOpen }) => (
          <div className="space-y-3">
            {/* Reboot dropdown */}
            <Select value="" onValueChange={handleReboot} disabled={!!rebooting}>
              <SelectTrigger
                className={sidebarOpen ? "w-full justify-start gap-2" : "w-full justify-center px-0"}
                size="sm"
              >
                <RotateCcw size={16} className={rebooting ? "animate-spin" : ""} />
                {sidebarOpen && "Reboot"}
              </SelectTrigger>
              <SelectContent>
                {rebootTargets.map((t) => (
                  <SelectItem key={t.value} value={t.value}>
                    {t.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>

            {/* Device status */}
            {sidebarOpen ? (
              <div className="panel-inset px-2.5 py-2 text-xs space-y-1.5">
                <div className="flex items-center gap-2">
                  <span className={`size-2 rounded-full ${connected ? "dot-complete" : "dot-waiting"}`} />
                  <span className="font-medium">{connected ? "Connected" : "Disconnected"}</span>
                  <button className="ml-auto" onClick={fetchDevice} disabled={deviceLoading}>
                    <RefreshCw size={12} className={deviceLoading ? "animate-spin" : ""} />
                  </button>
                </div>
                {connected && (
                  <div className="text-muted-foreground space-y-0.5">
                    <div>
                      <span className="tracking-[0.1em] uppercase">Serial</span>{" "}
                      {device?.serial ?? "—"}
                    </div>
                    <div>
                      <span className="tracking-[0.1em] uppercase">Product</span>{" "}
                      {vars.product ?? "—"}
                    </div>
                    <div>
                      <span className="tracking-[0.1em] uppercase">Slot</span>{" "}
                      {vars["current-slot"] ?? "—"}
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
                <span className={`size-2 rounded-full ${connected ? "dot-complete" : "dot-waiting"}`} />
              </Button>
            )}
          </div>
        )}
      >
        {({ tab }) => (
          <Suspense fallback={null}>
            {tab === "main" && <MainTab device={device} onRefresh={fetchDevice} />}
            {tab === "tools" && <ToolsTab />}
            {tab === "settings" && <SettingsTab />}
          </Suspense>
        )}
      </AppLayout>
      <Toaster richColors position="top-center" theme={theme} />
    </>
  );
}

export default App;
