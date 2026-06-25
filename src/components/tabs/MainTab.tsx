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
  Copy,
  LoaderCircle,
} from "lucide-react";
import type { DeviceInfo } from "@/types/api";

interface ConfirmAction {
  title: string;
  description: string;
  confirmLabel?: string;
  variant?: "destructive" | "default";
  onConfirm: () => void;
}

interface MainTabProps {
  device: DeviceInfo | null;
  onRefresh: () => Promise<void>;
}

export default function MainTab({ device, onRefresh }: MainTabProps) {
  const [connecting, setConnecting] = useState(false);
  const [lockLoading, setLockLoading] = useState(false);
  const [unlockLoading, setUnlockLoading] = useState(false);
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
    setLockLoading(true);
    try {
      await invoke<string>("lock_bootloader");
      toast.success("Bootloader locked");
      await onRefresh();
    } catch (e) {
      toast.error(`Lock failed: ${e}`);
    }
    setLockLoading(false);
  };

  const handleUnlock = async () => {
    setUnlockLoading(true);
    try {
      await invoke<string>("unlock_bootloader");
      toast.success("Bootloader unlocked");
      await onRefresh();
    } catch (e) {
      toast.error(`Unlock failed: ${e}`);
    }
    setUnlockLoading(false);
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
    if (!varName.trim()) {
      toast.error("Enter a variable name");
      return;
    }
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
        <div className="flex items-start gap-5 px-5 py-5">
          <span className="flex size-10 shrink-0 items-center justify-center rounded-md bg-accent-brand/10 text-accent-brand">
            <Zap size={20} />
          </span>
          <div className="min-w-0 flex-1">
            <h2 className="text-body font-semibold text-foreground">
              Force Fastboot
            </h2>
            <p className="mt-1 text-label text-muted-foreground leading-normal max-w-lg">
              Force a MediaTek device into fastboot mode via preloader serial handshake.
            </p>
            <div className="mt-3 flex items-center gap-3">
              <Button
                size="default"
                onClick={() =>
                  setConfirmDialog({
                    title: "Force Fastboot",
                    description:
                      "This will attempt to force your MediaTek device into fastboot mode via preloader serial handshake. Ensure the device is powered off and connected via USB.",
                    confirmLabel: "Force",
                    variant: "default",
                    onConfirm: forceFastboot,
                  })
                }
                disabled={connecting}
              >
                {connecting ? <><LoaderCircle size={14} className="animate-spin" /> Connecting...</> : "Force Fastboot"}
              </Button>
              <span className={`size-1.5 rounded-full transition-colors duration-300 ${connected ? "dot-complete" : "dot-waiting animate-pulse"}`} />
              <span className="text-caption text-muted-foreground">
                {connected ? "Device online" : "No device"}
              </span>
            </div>
          </div>
        </div>
        {/* Connected device info strip */}
        {connected && (
          <div className="animate-in fade-in slide-in-from-top-1 duration-200 border-t border-border/50 px-5 py-2.5 flex items-center gap-4 text-caption text-muted-foreground/80 bg-success/[0.08]">
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
      <section className="panel-shell flex items-center justify-between gap-5 px-5 py-3 max-sm:flex-wrap">
        <div className="flex items-center gap-3 min-w-0">
          <Lock size={16} className="shrink-0 text-muted-foreground" />
          <span className="text-body font-medium text-foreground/90">Bootloader</span>
        </div>
        <div className="flex items-center gap-1.5">
          <Button
            variant="ghost"
            size="sm"
            onClick={() =>
              setConfirmDialog({
                title: "Lock Bootloader",
                description:
                  "Locking the bootloader will re-enable verified boot. This may prevent flashing custom firmware. Continue?",
                confirmLabel: "Lock",
                onConfirm: handleLock,
              })
            }
            disabled={lockLoading || !connected}
          >
          {lockLoading ? <><LoaderCircle size={14} className="animate-spin" /> Locking...</> : <><Lock size={14} className="mr-1" /> Lock</>}
            </Button>
            <Button
              variant="ghost"
              size="sm"
              onClick={() =>
                setConfirmDialog({
                  title: "Unlock Bootloader",
                  description:
                    "Unlocking the bootloader will disable verified boot and may wipe user data. Continue?",
                  confirmLabel: "Unlock",
                  onConfirm: handleUnlock,
                })
              }
              disabled={unlockLoading || !connected}
            >
              {unlockLoading ? <><LoaderCircle size={14} className="animate-spin" /> Unlocking...</> : <><Unlock size={14} className="mr-1" /> Unlock</>}
          </Button>
        </div>
      </section>

      {/* Active slot */}
      <section className="panel-shell flex items-center justify-between gap-5 px-5 py-3">
        <div className="flex items-center gap-3 min-w-0">
          <Cpu size={16} className="shrink-0 text-muted-foreground" />
          <span className="text-body font-medium text-foreground/90">Active Slot</span>
        </div>
        <div className="flex rounded-lg border border-border overflow-hidden">
          {["a", "b"].map((slot) => (
            <Button
              key={slot}
              variant="ghost"
              size="sm"
              onClick={() => handleSetSlot(slot)}
              disabled={!connected}
              className={`rounded-none ${
                vars["current-slot"] === slot
                  ? "bg-accent-brand text-accent-brand-foreground hover:bg-accent-brand/90"
                  : "text-muted-foreground hover:text-foreground hover:bg-muted/40"
              }`}
            >
              {slot.toUpperCase()}
            </Button>
          ))}
        </div>
      </section>

      {/* Get variable */}
      <section className="panel-shell px-5 py-3">
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
                <Search size={16} />
          </Button>
        </div>
        {varResult && (
          <div className="animate-in fade-in slide-in-from-top-1 duration-200 mt-2 flex items-start gap-2 rounded border border-border/50 bg-muted/30 px-2.5 py-1.5">
            <code className="flex-1 font-mono text-label text-foreground/80 min-w-0 break-all">{varResult}</code>
            <Button
              variant="ghost"
              size="icon-xs"
              className="shrink-0 mt-0.5"
              onClick={() => {
                navigator.clipboard.writeText(varResult);
                toast.success("Copied to clipboard");
              }}
              aria-label="Copy value"
            >
              <Copy size={14} />
            </Button>
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
          variant={confirmDialog.variant ?? "destructive"}
          onConfirm={confirmDialog.onConfirm}
        />
      )}
    </div>
  );
}
