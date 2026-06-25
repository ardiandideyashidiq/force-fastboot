import { useState, lazy, Suspense, useCallback } from "react";
import AppLayout from "@/components/layout/AppLayout";
import { Toaster } from "sonner";
import type { Theme } from "@/types/api";

const MainTab = lazy(() => import("@/components/tabs/MainTab"));
const ToolsTab = lazy(() => import("@/components/tabs/ToolsTab"));
const SettingsTab = lazy(() => import("@/components/tabs/SettingsTab"));

function App() {
  const [theme, setTheme] = useState<Theme>(() => {
    const root = document.documentElement;
    return root.classList.contains("dark") ? "dark" : "light";
  });

  const onThemeChange = useCallback(
    (value: Theme | ((current: Theme) => Theme)) => {
      setTheme(value);
    },
    [],
  );

  return (
    <>
      <AppLayout
        theme={theme}
        onThemeChange={onThemeChange}
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
