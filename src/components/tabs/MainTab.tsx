import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Separator } from "@/components/ui/separator";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  RefreshCw,
  Zap,
  Lock,
  Unlock,
  Cpu,
  Search,
  Smartphone,
} from "lucide-react";

interface DeviceInfo {
  connected: boolean;
  serial: string | null;
  vars: Record<string, string>;
}

export default function MainTab() {
  const [device, setDevice] = useState<DeviceInfo | null>(null);
  const [loading, setLoading] = useState(false);
  const [connecting, setConnecting] = useState(false);
  const [locking, setLocking] = useState(false);
  const [varName, setVarName] = useState("");
  const [varResult, setVarResult] = useState("");
  const [varLoading, setVarLoading] = useState(false);

  const fetchDevice = useCallback(async () => {
    setLoading(true);
    try {
      const info = await invoke<DeviceInfo>("get_device_info");
      setDevice(info);
    } catch (e) {
      console.error("Failed to get device info:", e);
    }
    setLoading(false);
  }, []);

  useEffect(() => {
    fetchDevice();
  }, [fetchDevice]);

  const forceFastboot = async () => {
    setConnecting(true);
    try {
      await invoke("force_fastboot");
      await fetchDevice();
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
      await fetchDevice();
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
      await fetchDevice();
    } catch (e) {
      console.error("Unlock failed:", e);
    }
    setLocking(false);
  };

  const handleSetSlot = async (slot: string) => {
    try {
      const resp = await invoke<string>("set_active_slot", { slot });
      console.log(`Slot ${slot} set:`, resp);
      await fetchDevice();
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

  const connected = device?.connected ?? false;
  const vars = device?.vars ?? {};

  return (
    <div className="space-y-6">
      {/* Device card */}
      <Card>
        <CardHeader className="flex flex-row items-center justify-between">
          <CardTitle className="flex items-center gap-2">
            <Smartphone size={20} />
            Device
          </CardTitle>
          <div className="flex items-center gap-2">
            <Badge variant={connected ? "default" : "secondary"}>
              {connected ? "Connected" : "Disconnected"}
            </Badge>
            <Button variant="outline" size="icon-sm" onClick={fetchDevice} disabled={loading}>
              <RefreshCw size={16} className={loading ? "animate-spin" : ""} />
            </Button>
          </div>
        </CardHeader>
        <CardContent>
          {connected ? (
            <div className="grid grid-cols-2 gap-3 text-sm">
              <div>
                <span className="text-muted-foreground">Serial:</span>{" "}
                {device?.serial ?? "—"}
              </div>
              <div>
                <span className="text-muted-foreground">Product:</span>{" "}
                {vars.product ?? "—"}
              </div>
              <div>
                <span className="text-muted-foreground">Version:</span>{" "}
                {vars.version ?? "—"}
              </div>
              <div>
                <span className="text-muted-foreground">Slot:</span>{" "}
                {vars["current-slot"] ?? "—"}
              </div>
            </div>
          ) : (
            <p className="text-sm text-muted-foreground">
              No fastboot device detected. Connect your device and try Force
              Fastboot.
            </p>
          )}
        </CardContent>
      </Card>

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
