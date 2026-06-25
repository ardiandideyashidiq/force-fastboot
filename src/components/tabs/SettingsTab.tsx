import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Info } from "lucide-react";

export default function SettingsTab() {
  return (
    <div className="space-y-6">
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Info size={20} />
            About
          </CardTitle>
        </CardHeader>
        <CardContent className="space-y-2 text-sm">
          <p>
            <span className="text-muted-foreground">pawflash</span> — MTK
            device flashing toolkit
          </p>
          <p>
            <span className="text-muted-foreground">Version:</span> 0.1.0
          </p>
          <p>
            <span className="text-muted-foreground">Stack:</span> Tauri v2 +
            React 19 + Rust
          </p>
        </CardContent>
      </Card>
    </div>
  );
}
