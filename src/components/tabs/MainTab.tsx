import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Separator } from "@/components/ui/separator";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Zap, Lock, Unlock, Cpu, Search } from "lucide-react";
import type { DeviceInfo } from "@/types/api";

interface MainTabProps {
  device: DeviceInfo | null;
  onRefresh: () => Promise<void>;
}

export default function MainTab({ device, onRefresh }: MainTabProps) {
  const [connecting, setConnecting] = useState(false);
  const [locking, setLocking] = useState(false);
  const [varName, setVarName] = useState("");
  const [varResult, setVarResult] = useState("");
  const [varLoading, setVarLoading] = useState(false);

  const connected = device?.connected ?? false;
  const vars = device?.vars ?? {};

  const forceFastboot = async () => {
    setConnecting(true);
    try {
      await invoke("force_fastboot");
      await onRefresh();
    } catch (e) {
      console.error("Force fastboot failed:", e);
    }
    setConnecting(false);
  };

  const handleLock = async () => {
    setLocking(true);
    try {
      const resp = await invoke<string>("lock_bootloader");
      console.log("Lock response:", resp);
      await onRefresh();
    } catch (e) {
      console.error("Lock failed:", e);
    }
    setLocking(false);
  };

  const handleUnlock = async () => {
    setLocking(true);
    try {
      const resp = await invoke<string>("unlock_bootloader");
      console.log("Unlock response:", resp);
      await onRefresh();
    } catch (e) {
      console.error("Unlock failed:", e);
    }
    setLocking(false);
  };

  const handleSetSlot = async (slot: string) => {
    try {
      const resp = await invoke<string>("set_active_slot", { slot });
      console.log(`Slot ${slot} set:`, resp);
      await onRefresh();
    } catch (e) {
      console.error(`Set slot ${slot} failed:`, e);
    }
  };

  const handleGetVar = async () => {
    if (!varName.trim()) return;
    setVarLoading(true);
    try {
      const value = await invoke<string>("get_var", { name: varName.trim() });
      setVarResult(value);
    } catch (e) {
      setVarResult(`Error: ${e}`);
    }
    setVarLoading(false);
  };

  return (
    <div className="space-y-6">
      {/* Force fastboot */}
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Zap size={20} />
            Force Fastboot
          </CardTitle>
        </CardHeader>
        <CardContent>
          <p className="text-sm text-muted-foreground mb-3">
            Force a MediaTek device into fastboot mode via preloader serial
            handshake.
          </p>
          <Button onClick={forceFastboot} disabled={connecting}>
            {connecting ? "Connecting..." : "Force Fastboot"}
          </Button>
        </CardContent>
      </Card>

      {/* Lock/Unlock */}
      <div className="flex flex-wrap gap-2 items-center">
        <span className="text-sm font-medium text-muted-foreground mr-2">
          Bootloader:
        </span>
        <Button
          variant="outline"
          size="sm"
          onClick={handleLock}
          disabled={locking || !connected}
        >
          <Lock size={14} className="mr-1" />
          Lock
        </Button>
        <Button
          variant="outline"
          size="sm"
          onClick={handleUnlock}
          disabled={locking || !connected}
        >
          <Unlock size={14} className="mr-1" />
          Unlock
        </Button>
      </div>

      <Separator />

      {/* Slot select */}
      <div className="flex flex-wrap gap-2 items-center">
        <span className="text-sm font-medium text-muted-foreground mr-2">
          Active slot:
        </span>
        {["a", "b"].map((slot) => (
          <Button
            key={slot}
            variant={
              vars["current-slot"] === slot ? "secondary" : "outline"
            }
            size="sm"
            onClick={() => handleSetSlot(slot)}
            disabled={!connected}
          >
            <Cpu size={14} className="mr-1" />
            Slot {slot.toUpperCase()}
          </Button>
        ))}
      </div>

      <Separator />

      {/* Get var */}
      <div className="flex items-end gap-3 max-w-md">
        <div className="flex-1 space-y-1">
          <Label htmlFor="var-name">Get variable</Label>
          <Input
            id="var-name"
            placeholder="e.g. max-download-size"
            value={varName}
            onChange={(e) => setVarName(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && handleGetVar()}
          />
        </div>
        <Button
          variant="outline"
          size="icon"
          onClick={handleGetVar}
          disabled={varLoading || !varName.trim() || !connected}
        >
          <Search size={16} />
        </Button>
      </div>
      {varResult && (
        <p className="text-sm font-mono mt-1 px-3 py-2 rounded bg-muted">
          {varResult}
        </p>
      )}
    </div>
  );
}
