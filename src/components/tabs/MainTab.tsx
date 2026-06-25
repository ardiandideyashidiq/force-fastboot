import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Zap,
  Lock,
  Unlock,
  Cpu,
  Search,
} from "lucide-react";
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
    <div className="space-y-5">
      {/* Force Fastboot — hero action */}
      <section className="panel-shell overflow-hidden">
        <div className="flex items-start gap-5 p-4">
          <span className="flex size-10 shrink-0 items-center justify-center rounded-md bg-accent-brand/10 text-accent-brand">
            <Zap size={20} />
          </span>
          <div className="min-w-0 flex-1">
            <h2 className="text-[0.8125rem] font-semibold text-foreground">
              Force Fastboot
            </h2>
            <p className="mt-1 text-[0.75rem] text-muted-foreground leading-relaxed max-w-md">
              Force a MediaTek device into fastboot mode via preloader serial handshake.
            </p>
            <div className="mt-3 flex items-center gap-3">
              <Button
                size="sm"
                onClick={forceFastboot}
                disabled={connecting}
              >
                {connecting ? "Connecting..." : "Force Fastboot"}
              </Button>
              <span className={`size-1.5 rounded-full ${connected ? "dot-complete" : "dot-waiting"}`} />
              <span className="text-[0.7rem] text-muted-foreground">
                {connected ? "Device online" : "No device"}
              </span>
            </div>
          </div>
        </div>
        {/* Connected device info strip */}
        {connected && (
          <div className="border-t border-border/50 px-4 py-2 flex items-center gap-4 text-[0.7rem] text-muted-foreground/80 bg-muted/20">
            <span className="font-mono text-accent-brand/70">{device?.serial ?? "—"}</span>
            <span className="w-px h-3 bg-border/50" />
            <span>{vars.product ?? "—"}</span>
            {vars["current-slot"] && (
              <>
                <span className="w-px h-3 bg-border/50" />
                <span>slot {vars["current-slot"]}</span>
              </>
            )}
          </div>
        )}
      </section>

      {/* Bootloader controls */}
      <section className="flex items-center justify-between gap-4 rounded-md border border-border/60 bg-card/80 px-4 py-3">
        <div className="flex items-center gap-3 min-w-0">
          <Lock size={14} className="shrink-0 text-muted-foreground" />
          <span className="text-[0.8125rem] font-medium text-foreground/90">Bootloader</span>
        </div>
        <div className="flex items-center gap-1.5">
          <Button
            variant="ghost"
            size="xs"
            onClick={handleLock}
            disabled={locking || !connected}
          >
            <Lock size={12} className="mr-1" />
            Lock
          </Button>
          <Button
            variant="ghost"
            size="xs"
            onClick={handleUnlock}
            disabled={locking || !connected}
          >
            <Unlock size={12} className="mr-1" />
            Unlock
          </Button>
        </div>
      </section>

      {/* Active slot */}
      <section className="flex items-center justify-between gap-4 rounded-md border border-border/60 bg-card/80 px-4 py-3">
        <div className="flex items-center gap-3 min-w-0">
          <Cpu size={14} className="shrink-0 text-muted-foreground" />
          <span className="text-[0.8125rem] font-medium text-foreground/90">Active Slot</span>
        </div>
        <div className="flex rounded-md border border-border/60 overflow-hidden">
          {["a", "b"].map((slot) => (
            <button
              key={slot}
              onClick={() => handleSetSlot(slot)}
              disabled={!connected}
              className={
                `px-3 py-1 text-[0.75rem] font-medium transition-colors disabled:opacity-40 disabled:cursor-not-allowed ` +
                (vars["current-slot"] === slot
                  ? "bg-accent-brand text-accent-brand-foreground"
                  : "bg-transparent text-muted-foreground hover:text-foreground hover:bg-muted/40")
              }
            >
              {slot.toUpperCase()}
            </button>
          ))}
        </div>
      </section>

      {/* Get variable */}
      <section className="rounded-md border border-border/60 bg-card/80 px-4 py-3">
        <Label
          htmlFor="var-name"
          className="text-[0.7rem] font-semibold uppercase tracking-[0.12em] text-muted-foreground mb-2 block"
        >
          Get Variable
        </Label>
        <div className="flex items-end gap-2 max-w-sm">
          <div className="flex-1">
            <Input
              id="var-name"
              placeholder="e.g. max-download-size"
              value={varName}
              onChange={(e) => setVarName(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && handleGetVar()}
              className="text-[0.8125rem]"
            />
          </div>
          <Button
            variant="outline"
            size="icon"
            onClick={handleGetVar}
            disabled={varLoading || !varName.trim() || !connected}
          >
            <Search size={14} />
          </Button>
        </div>
        {varResult && (
          <div className="mt-2 rounded border border-border/50 bg-muted/30 px-2.5 py-1.5">
            <code className="font-mono text-[0.75rem] text-foreground/80">{varResult}</code>
          </div>
        )}
      </section>
    </div>
  );
}
