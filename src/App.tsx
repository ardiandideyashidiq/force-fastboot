import { useState, lazy, Suspense, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import AppLayout from "@/components/layout/AppLayout";
import { Select, SelectTrigger, SelectContent, SelectItem } from "@/components/ui/select";
import { Toaster } from "sonner";
import { RotateCcw } from "lucide-react";
import type { Theme } from "@/types/api";

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

  const onThemeChange = useCallback(
    (value: Theme | ((current: Theme) => Theme)) => {
      setTheme(value);
    },
    [],
  );

  const handleReboot = useCallback(async (target: string | null) => {
    if (!target) return;
    setRebooting(target);
    try {
      await invoke("reboot_device", { target });
    } catch (e) {
      console.error("Reboot failed:", e);
    }
    setRebooting(null);
  }, []);

  return (
    <>
      <AppLayout
        theme={theme}
        onThemeChange={onThemeChange}
        sidebarActions={({ sidebarOpen }) => (
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
        )}
      >
        {({ tab }) => (
          <Suspense fallback={null}>
            {tab === "main" && <MainTab />}
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
