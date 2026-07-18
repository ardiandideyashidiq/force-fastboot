import { useState, useMemo } from "react";
import { invoke, Channel } from "@tauri-apps/api/core";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Select, SelectTrigger, SelectContent, SelectItem } from "@/components/ui/select";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { useConsole } from "@/hooks/useConsole";
import type { ScatterFile, ConfirmAction } from "@/types/api";
import type { ProgressEvent } from "@/types/progress";
import {
  FileText,
  Upload,
  Play,
  Trash2,
  ShieldOff,
  HardDrive,
  LoaderCircle,
} from "lucide-react";

export default function ToolsTab() {
  const { addProgressEvent } = useConsole();
  const [scatterPath, setScatterPath] = useState("");
  const [scatterMeta, setScatterMeta] = useState<ScatterFile | null>(null);
  const [scatterLoading, setScatterLoading] = useState(false);
  const [vbmetaLoading, setVbmetaLoading] = useState(false);
  const [formatLoading, setFormatLoading] = useState(false);
  const [planLoading, setPlanLoading] = useState(false);
  const [formatFsType, setFormatFsType] = useState("f2fs");
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

  const handleDisableVbmeta = async () => {
    setVbmetaLoading(true);
    try {
      const channel = new Channel<ProgressEvent>();
      channel.onmessage = addProgressEvent;
      await invoke("disable_vbmeta", { onEvent: channel });
      toast.success("Verified boot disabled");
    } catch (e) {
      toast.error(`Failed to disable verified boot: ${e}`);
    }
    setVbmetaLoading(false);
  };

  const handleFormatData = async () => {
    setFormatLoading(true);
    try {
      const channel = new Channel<ProgressEvent>();
      channel.onmessage = addProgressEvent;
      await invoke("format_data", {
        fsType: formatFsType,
        fsOptions: [] as string[],
        cleanTest: false,
        onEvent: channel,
      });
      toast.success("User data formatted");
    } catch (e) {
      toast.error(`Format failed: ${e}`);
    }
    setFormatLoading(false);
  };

  const partitionCount = useMemo(
    () => scatterMeta ? Object.values(scatterMeta.layouts).reduce((sum, parts) => sum + parts.length, 0) : 0,
    [scatterMeta],
  );
  const layoutNames = useMemo(
    () => scatterMeta ? Object.keys(scatterMeta.layouts) : [],
    [scatterMeta],
  );

  const handleExecutePlan = async () => {
    if (!scatterPath) return;
    setPlanLoading(true);
    try {
      const channel = new Channel<ProgressEvent>();
      channel.onmessage = addProgressEvent;
      await invoke("execute_plan", {
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
        onEvent: channel,
      });
      toast.success("Flash plan executed");
    } catch (e) {
      toast.error(`Flash plan failed: ${e}`);
    }
    setPlanLoading(false);
  };

  return (
    <div className="space-y-5">
      {/* Scatter File */}
      <section className="panel-shell overflow-hidden">
        <div className="flex items-start gap-4 px-5 py-5">
          <span className="flex size-10 shrink-0 items-center justify-center rounded-md bg-trace-copper/10 text-trace-copper">
            <FileText size={18} />
          </span>
          <div className="min-w-0 flex-1">
            <h2 className="text-body font-display font-medium uppercase tracking-wider text-foreground">
              Scatter File
            </h2>
            <div className="mt-2 flex items-center gap-2">
              <Button variant="outline" size="sm" onClick={pickScatter}>
                <Upload size={14} className="mr-1" />
                Select
              </Button>
              {scatterPath && (
                <span className="text-caption font-mono text-muted-foreground truncate max-w-[20rem] max-sm:max-w-full">
                  {scatterPath}
                </span>
              )}
            </div>

            {scatterLoading && (
              <p className="mt-2 text-label text-muted-foreground">
                <LoaderCircle size={14} className="animate-spin inline mr-1" />
                Parsing...
              </p>
            )}

            {scatterMeta && (
              <div className="animate-in fade-in slide-in-from-top-1 duration-200 mt-3 space-y-3">
                <div className="grid grid-cols-3 max-sm:grid-cols-2 gap-x-4 gap-y-1.5 text-label">
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
                </div>
                  <Button
                    variant="accent"
                    size="sm"
                    onClick={handleExecutePlan}
                    disabled={planLoading}
                  >
                    {planLoading ? <><LoaderCircle size={14} className="animate-spin" /> Executing...</> : <><Play size={14} className="mr-1" /> Execute Flash Plan</>}
                </Button>
              </div>
            )}
          </div>
        </div>
      </section>

      {/* Format User Data + Disable Verified Boot */}
      <div className="grid grid-cols-2 max-sm:grid-cols-1 gap-3">
        <section className="panel-shell flex items-center justify-between gap-3 px-5 py-3">
          <div className="flex items-center gap-3 min-w-0">
            <HardDrive size={16} className="shrink-0 text-muted-foreground" />
            <div>
              <p className="text-body font-display font-medium uppercase tracking-wider text-foreground/90">
                Format User Data
              </p>
              <p className="text-caption text-muted-foreground/70 leading-tight">
                {formatFsType.toUpperCase()}
              </p>
            </div>
          </div>
          <div className="flex items-center gap-1.5 shrink-0">
            <Select value={formatFsType} onValueChange={(v) => v && setFormatFsType(v)}>
              <SelectTrigger size="sm" className="h-8 min-w-16">
                <span className="text-label">{formatFsType.toUpperCase()}</span>
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="f2fs">F2FS</SelectItem>
                <SelectItem value="ext4">Ext4</SelectItem>
              </SelectContent>
            </Select>
            <Button
              variant="accent"
              size="sm"
              onClick={() =>
                setConfirmDialog({
                  title: "Format User Data",
                  description:
                    "This will erase all user data on the device. The data partition will be reformatted and all contents will be lost. Continue?",
                  confirmLabel: "Format",
                  onConfirm: handleFormatData,
                })
              }
              disabled={formatLoading}
            >
        <Trash2 size={14} className="mr-1" />
            {formatLoading ? <><LoaderCircle size={14} className="animate-spin" /> Working...</> : "Format"}
            </Button>
          </div>
        </section>

        <section className="panel-shell flex items-center justify-between gap-3 px-5 py-3">
          <div className="flex items-center gap-3 min-w-0">
            <ShieldOff size={16} className="shrink-0 text-muted-foreground" />
            <div>
              <p className="text-body font-display font-medium uppercase tracking-wider text-foreground/90">
                Disable Verified Boot
              </p>
              <p className="text-caption text-muted-foreground/70 leading-tight">
                dm-verity + AVB
              </p>
            </div>
          </div>
          <Button
            variant="destructive"
            size="sm"
            onClick={() =>
              setConfirmDialog({
                title: "Disable Verified Boot",
                description:
                  "Disabling dm-verity and AVB will weaken device security verification. This is typically needed only when flashing custom firmware. Continue?",
                confirmLabel: "Disable",
                onConfirm: handleDisableVbmeta,
              })
            }
            disabled={vbmetaLoading}
          >
            {vbmetaLoading ? <><LoaderCircle size={14} className="animate-spin" /> Working...</> : "Disable"}
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
