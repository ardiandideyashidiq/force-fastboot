import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import {
  Zap,
  Lock,
  Unlock,
  Cpu,
  Search,
} from "lucide-react";
import type { DeviceInfo } from "@/types/api";

interface ConfirmAction {
  title: string;
  description: string;
  confirmLabel?: string;
  onConfirm: () => void;
}

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
  const [confirmDialog, setConfirmDialog] = useState<ConfirmAction | null>(null);

  const connected = device?.connected ?? false;
  const vars = device?.vars ?? {};

  const forceFastboot = async () => {
    setConnecting(true);
    try {
      await invoke("force_fastboot");
      await onRefresh();
    } catch (e) {
      toast.error(`Force fastboot failed: ${e}`);
    }
    setConnecting(false);
  };

  const handleLock = async () => {
    setLocking(true);
    try {
      await invoke<string>("lock_bootloader");
      toast.success("Bootloader locked");
      await onRefresh();
    } catch (e) {
      toast.error(`Lock failed: ${e}`);
    }
    setLocking(false);
  };

  const handleUnlock = async () => {
    setLocking(true);
    try {
      await invoke<string>("unlock_bootloader");
      toast.success("Bootloader unlocked");
      await onRefresh();
    } catch (e) {
      toast.error(`Unlock failed: ${e}`);
    }
    setLocking(false);
  };

  const handleSetSlot = async (slot: string) => {
    try {
      await invoke<string>("set_active_slot", { slot });
      toast.success(`Slot ${slot} set`);
      await onRefresh();
    } catch (e) {
      toast.error(`Set slot ${slot} failed: ${e}`);
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
            <h2 className="text-body font-semibold text-foreground">
              Force Fastboot
            </h2>
            <p className="mt-1 text-label text-muted-foreground leading-relaxed max-w-md">
              Force a MediaTek device into fastboot mode via preloader serial handshake.
            </p>
            <div className="mt-3 flex items-center gap-3">
              <Button
                size="sm"
                onClick={() =>
                  setConfirmDialog({
                    title: "Force Fastboot",
                    description:
                      "This will attempt to force your MediaTek device into fastboot mode via preloader serial handshake. Ensure the device is powered off and connected via USB.",
                    confirmLabel: "Force",
                    onConfirm: forceFastboot,
                  })
                }
                disabled={connecting}
              >
                {connecting ? "Connecting..." : "Force Fastboot"}
              </Button>
              <span className={`size-1.5 rounded-full ${connected ? "dot-complete" : "dot-waiting"}`} />
              <span className="text-caption text-muted-foreground">
                {connected ? "Device online" : "No device"}
              </span>
            </div>
          </div>
        </div>
        {/* Connected device info strip */}
        {connected && (
          <div className="border-t border-border/50 px-4 py-2 flex items-center gap-4 text-caption text-muted-foreground/80 bg-muted/20">
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
      <section className="panel-shell flex items-center justify-between gap-4 px-4 py-3">
        <div className="flex items-center gap-3 min-w-0">
          <Lock size={14} className="shrink-0 text-muted-foreground" />
          <span className="text-body font-medium text-foreground/90">Bootloader</span>
        </div>
        <div className="flex items-center gap-1.5">
          <Button
            variant="ghost"
            size="xs"
            onClick={() =>
              setConfirmDialog({
                title: "Lock Bootloader",
                description:
                  "Locking the bootloader will re-enable verified boot. This may prevent flashing custom firmware. Continue?",
                confirmLabel: "Lock",
                onConfirm: handleLock,
              })
            }
            disabled={locking || !connected}
          >
            <Lock size={12} className="mr-1" />
            Lock
          </Button>
          <Button
            variant="ghost"
            size="xs"
            onClick={() =>
              setConfirmDialog({
                title: "Unlock Bootloader",
                description:
                  "Unlocking the bootloader will disable verified boot and may wipe user data. Continue?",
                confirmLabel: "Unlock",
                onConfirm: handleUnlock,
              })
            }
            disabled={locking || !connected}
          >
            <Unlock size={12} className="mr-1" />
            Unlock
          </Button>
        </div>
      </section>

      {/* Active slot */}
      <section className="panel-shell flex items-center justify-between gap-4 px-4 py-3">
        <div className="flex items-center gap-3 min-w-0">
          <Cpu size={14} className="shrink-0 text-muted-foreground" />
          <span className="text-body font-medium text-foreground/90">Active Slot</span>
        </div>
        <div className="flex rounded-lg border border-border overflow-hidden">
          {["a", "b"].map((slot) => (
            <Button
              key={slot}
              variant="ghost"
              size="xs"
              onClick={() => handleSetSlot(slot)}
              disabled={!connected}
              className={`rounded-none ${
                vars["current-slot"] === slot
                  ? "bg-accent-brand text-accent-brand-foreground hover:bg-accent-brand"
                  : "text-muted-foreground hover:text-foreground hover:bg-muted/40"
              }`}
            >
              {slot.toUpperCase()}
            </Button>
          ))}
        </div>
      </section>

      {/* Get variable */}
      <section className="panel-shell px-4 py-3">
        <Label
          htmlFor="var-name"
          className="text-caption font-semibold uppercase tracking-label text-muted-foreground mb-2 block"
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
              className="text-body"
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
            <code className="font-mono text-label text-foreground/80">{varResult}</code>
          </div>
        )}
      </section>

      {confirmDialog && (
        <ConfirmDialog
          open={!!confirmDialog}
          onOpenChange={(open) => {
            if (!open) setConfirmDialog(null);
          }}
          title={confirmDialog.title}
          description={confirmDialog.description}
          confirmLabel={confirmDialog.confirmLabel}
          variant="destructive"
          onConfirm={confirmDialog.onConfirm}
        />
      )}
    </div>
  );
}
