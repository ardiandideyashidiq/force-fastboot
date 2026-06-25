import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Select, SelectTrigger, SelectContent, SelectItem } from "@/components/ui/select";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import {
  FileText,
  Upload,
  Play,
  Trash2,
  ShieldOff,
  HardDrive,
  Gauge,
} from "lucide-react";
import type { ScatterFile } from "@/types/api";

interface ConfirmAction {
  title: string;
  description: string;
  confirmLabel?: string;
  onConfirm: () => void;
}

export default function ToolsTab() {
  const [scatterPath, setScatterPath] = useState("");
  const [scatterMeta, setScatterMeta] = useState<ScatterFile | null>(null);
  const [scatterLoading, setScatterLoading] = useState(false);
  const [vbmetaLoading, setVbmetaLoading] = useState(false);
  const [formatLoading, setFormatLoading] = useState(false);
  const [formatFsType, setFormatFsType] = useState("f2fs");
  const [gsiPath, setGsiPath] = useState("");
  const [gsiLoading, setGsiLoading] = useState(false);
  const [confirmDialog, setConfirmDialog] = useState<ConfirmAction | null>(null);

  const pickScatter = async () => {
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const selected = await open({
        multiple: false,
        filters: [
          { name: "Scatter", extensions: ["txt", "xml", "yaml"] },
        ],
      });
      if (selected) {
        setScatterPath(selected as string);
        setScatterLoading(true);
        try {
          const meta = await invoke<ScatterFile>("parse_scatter", {
            path: selected as string,
          });
          setScatterMeta(meta);
        } catch (e) {
          toast.error(`Failed to parse scatter: ${e}`);
        }
        setScatterLoading(false);
      }
    } catch (e) {
      toast.error(`File dialog error: ${e}`);
    }
  };

  const pickGsi = async () => {
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const selected = await open({
        multiple: false,
        filters: [
          { name: "GSI Image", extensions: ["img"] },
        ],
      });
      if (selected) {
        setGsiPath(selected as string);
      }
    } catch (e) {
      toast.error(`File dialog error: ${e}`);
    }
  };

  const handleDisableVbmeta = async () => {
    setVbmetaLoading(true);
    try {
      await invoke("disable_vbmeta");
      toast.success("AVB/verity disabled");
    } catch (e) {
      toast.error(`Failed to disable AVB: ${e}`);
    }
    setVbmetaLoading(false);
  };

  const handleFormatData = async () => {
    setFormatLoading(true);
    try {
      await invoke("format_data", {
        fsType: formatFsType,
      });
      toast.success("Data partition formatted");
    } catch (e) {
      toast.error(`Format failed: ${e}`);
    }
    setFormatLoading(false);
  };

  const handleFlashGsi = async () => {
    if (!gsiPath) return;
    setGsiLoading(true);
    try {
      await invoke("flash_gsi", {
        imagePath: gsiPath,
        cleanTest: false,
      });
      toast.success("GSI flash complete");
    } catch (e) {
      toast.error(`GSI flash failed: ${e}`);
    }
    setGsiLoading(false);
  };

  const handleExecutePlan = () => {
    if (!scatterPath) return;
    invoke("execute_plan", {
      path: scatterPath,
      options: {
        mode: "selective",
        storage: "auto",
        parts: [],
        groups: [],
        firmware_dir: null,
        check_images: false,
        include_preloader: false,
      },
    });
  };

  return (
    <div className="space-y-5">
      {/* Scatter File */}
      <section className="panel-shell overflow-hidden">
        <div className="flex items-start gap-3 p-4">
          <span className="flex size-8 shrink-0 items-center justify-center rounded-md bg-accent-soft text-accent-soft-foreground">
            <FileText size={16} />
          </span>
          <div className="min-w-0 flex-1">
            <h2 className="text-body font-semibold text-foreground">
              Scatter File
            </h2>
            <div className="mt-2 flex items-center gap-2">
              <Button variant="outline" size="xs" onClick={pickScatter}>
                <Upload size={12} className="mr-1" />
                Select
              </Button>
              {scatterPath && (
                <span className="text-caption font-mono text-muted-foreground truncate max-w-[20rem]">
                  {scatterPath}
                </span>
              )}
            </div>

            {scatterLoading && (
              <p className="mt-2 text-label text-muted-foreground">Parsing...</p>
            )}

            {scatterMeta && (
              <div className="mt-3 space-y-3">
                <div className="grid grid-cols-3 gap-x-4 gap-y-1.5 text-label">
                  <div>
                    <span className="text-muted-foreground">Platform</span>
                    <p className="font-medium">{scatterMeta.platform ?? "—"}</p>
                  </div>
                  <div>
                    <span className="text-muted-foreground">Project</span>
                    <p className="font-medium">{scatterMeta.project ?? "—"}</p>
                  </div>
                  <div>
                    <span className="text-muted-foreground">Format</span>
                    <p className="font-medium">{scatterMeta.format}</p>
                  </div>
                  {(() => {
                    const partitionCount = Object.values(scatterMeta.layouts).reduce(
                      (sum, parts) => sum + parts.length, 0,
                    );
                    const layoutNames = Object.keys(scatterMeta.layouts);
                    return (
                      <>
                        <div>
                          <span className="text-muted-foreground">Partitions</span>
                          <p className="font-medium">{partitionCount}</p>
                        </div>
                        <div className="col-span-2">
                          <span className="text-muted-foreground">Layouts</span>
                          <p className="font-medium truncate">{layoutNames.join(", ")}</p>
                        </div>
                      </>
                    );
                  })()}
                </div>
                <Button
                  variant="default"
                  size="xs"
                  onClick={handleExecutePlan}
                >
                  <Play size={12} className="mr-1" />
                  Execute Flash Plan
                </Button>
              </div>
            )}
          </div>
        </div>
      </section>

      {/* GSI Flash */}
      <section className="panel-shell flex items-center justify-between gap-3 px-4 py-3">
        <div className="flex items-center gap-3 min-w-0">
          <Gauge size={14} className="shrink-0 text-muted-foreground" />
          <div className="min-w-0">
            <p className="text-body font-medium text-foreground/90">GSI Flash</p>
            {gsiPath && (
              <p className="text-caption font-mono text-muted-foreground truncate max-w-[14rem]">
                {gsiPath}
              </p>
            )}
          </div>
        </div>
        <div className="flex items-center gap-1.5 shrink-0">
          <Button variant="ghost" size="xs" onClick={pickGsi}>
            <Upload size={12} className="mr-1" />
            Select
          </Button>
          <Button
            variant="default"
            size="xs"
            onClick={handleFlashGsi}
            disabled={!gsiPath || gsiLoading}
          >
            {gsiLoading ? "Flashing..." : "Flash"}
          </Button>
        </div>
      </section>

      {/* Format Data + Disable AVB */}
      <div className="grid grid-cols-2 gap-3">
        <section className="panel-shell flex items-center justify-between gap-3 px-4 py-3">
          <div className="flex items-center gap-3 min-w-0">
            <HardDrive size={14} className="shrink-0 text-muted-foreground" />
            <span className="text-body font-medium text-foreground/90">Format Data</span>
          </div>
          <div className="flex items-center gap-1.5 shrink-0">
            <Select value={formatFsType} onValueChange={(v) => v && setFormatFsType(v)}>
              <SelectTrigger size="sm" className="h-7 min-w-16">
                <span className="text-label">{formatFsType.toUpperCase()}</span>
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="f2fs">F2FS</SelectItem>
                <SelectItem value="ext4">Ext4</SelectItem>
              </SelectContent>
            </Select>
            <Button
              variant="default"
              size="xs"
              onClick={() =>
                setConfirmDialog({
                  title: "Format Data",
                  description:
                    "This will erase all user data on the device. The data partition will be reformatted and all contents will be lost. Continue?",
                  confirmLabel: "Format",
                  onConfirm: handleFormatData,
                })
              }
              disabled={formatLoading}
            >
              <Trash2 size={12} className="mr-1" />
              {formatLoading ? "Working..." : "Format"}
            </Button>
          </div>
        </section>

        <section className="panel-shell flex items-center justify-between gap-3 px-4 py-3">
          <div className="flex items-center gap-3 min-w-0">
            <ShieldOff size={14} className="shrink-0 text-muted-foreground" />
            <div>
              <p className="text-body font-medium text-foreground/90">Disable AVB</p>
              <p className="text-caption text-muted-foreground/70 leading-tight">
                dm-verity + AVB
              </p>
            </div>
          </div>
          <Button
            variant="destructive"
            size="xs"
            onClick={() =>
              setConfirmDialog({
                title: "Disable AVB",
                description:
                  "Disabling dm-verity and AVB will weaken device security verification. This is typically needed only when flashing custom firmware. Continue?",
                confirmLabel: "Disable",
                onConfirm: handleDisableVbmeta,
              })
            }
            disabled={vbmetaLoading}
          >
            {vbmetaLoading ? "Working..." : "Disable"}
          </Button>
        </section>
      </div>

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
