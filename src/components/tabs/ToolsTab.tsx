import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Label } from "@/components/ui/label";
import { Separator } from "@/components/ui/separator";
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

export default function ToolsTab() {
  const [scatterPath, setScatterPath] = useState("");
  const [scatterMeta, setScatterMeta] = useState<ScatterFile | null>(null);
  const [scatterLoading, setScatterLoading] = useState(false);
  const [vbmetaLoading, setVbmetaLoading] = useState(false);
  const [formatLoading, setFormatLoading] = useState(false);
  const [formatFsType, setFormatFsType] = useState("f2fs");
  const [gsiPath, setGsiPath] = useState("");
  const [gsiLoading, setGsiLoading] = useState(false);

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
          console.error("Scatter parse failed:", e);
        }
        setScatterLoading(false);
      }
    } catch (e) {
      console.error("Dialog error:", e);
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
      console.error("Dialog error:", e);
    }
  };

  const handleDisableVbmeta = async () => {
    setVbmetaLoading(true);
    try {
      await invoke("disable_vbmeta");
    } catch (e) {
      console.error("Disable vbmeta failed:", e);
    }
    setVbmetaLoading(false);
  };

  const handleFormatData = async () => {
    setFormatLoading(true);
    try {
      await invoke("format_data", {
        fsType: formatFsType,
      });
    } catch (e) {
      console.error("Format data failed:", e);
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
    } catch (e) {
      console.error("GSI flash failed:", e);
    }
    setGsiLoading(false);
  };

  return (
    <div className="space-y-6">
      {/* Scatter */}
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <FileText size={20} />
            Scatter File
          </CardTitle>
        </CardHeader>
        <CardContent className="space-y-3">
          <Button variant="outline" onClick={pickScatter}>
            <Upload size={16} className="mr-2" />
            Select Scatter File
          </Button>
          {scatterPath && (
            <p className="text-sm font-mono text-muted-foreground truncate">
              {scatterPath}
            </p>
          )}
          {scatterLoading && (
            <p className="text-sm text-muted-foreground">Parsing...</p>
          )}
          {scatterMeta && (() => {
            const layoutNames = Object.keys(scatterMeta.layouts);
            const partitionCount = Object.values(scatterMeta.layouts).reduce(
              (sum, parts) => sum + parts.length, 0,
            );
            const chipset = scatterMeta.platform ?? scatterMeta.project ?? null;
            return (
              <div className="grid grid-cols-2 gap-2 text-sm">
                <div>
                  <span className="text-muted-foreground">Platform:</span>{" "}
                  {scatterMeta.platform ?? "—"}
                </div>
                <div>
                  <span className="text-muted-foreground">Project:</span>{" "}
                  {scatterMeta.project ?? "—"}
                </div>
                <div>
                  <span className="text-muted-foreground">Chipset:</span>{" "}
                  {chipset ?? "—"}
                </div>
                <div>
                  <span className="text-muted-foreground">Partitions:</span>{" "}
                  {partitionCount}
                </div>
                <div>
                  <span className="text-muted-foreground">Format:</span>{" "}
                  {scatterMeta.format}
                </div>
                <div>
                  <span className="text-muted-foreground">Layouts:</span>{" "}
                  {layoutNames.join(", ")}
                </div>
              </div>
            );
          })()}
          {scatterMeta && (
            <Button
              variant="default"
              size="sm"
              onClick={() => {
                invoke("execute_plan", {
                  path: scatterPath,
                  options: { mode: "selective", storage: "auto", parts: [], groups: [], firmware_dir: null, check_images: false, include_preloader: false },
                });
              }}
            >
              <Play size={16} className="mr-2" />
              Execute Flash Plan
            </Button>
          )}
        </CardContent>
      </Card>

      <Separator />

      {/* GSI Flash */}
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Gauge size={20} />
            GSI Flash
          </CardTitle>
        </CardHeader>
        <CardContent className="space-y-3">
          <Button variant="outline" onClick={pickGsi}>
            <Upload size={16} className="mr-2" />
            Select GSI Image
          </Button>
          {gsiPath && (
            <p className="text-sm font-mono text-muted-foreground truncate">
              {gsiPath}
            </p>
          )}
          <Button
            variant="default"
            onClick={handleFlashGsi}
            disabled={!gsiPath || gsiLoading}
          >
            {gsiLoading ? "Flashing..." : "Flash GSI"}
          </Button>
        </CardContent>
      </Card>

      <Separator />

      {/* Format Data */}
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <HardDrive size={20} />
            Format Data
          </CardTitle>
        </CardHeader>
        <CardContent className="space-y-3">
          <div className="flex items-center gap-3">
            <Label htmlFor="fs-type" className="text-sm">
              Filesystem:
            </Label>
            <select
              id="fs-type"
              className="rounded-md border border-input bg-background px-3 py-1 text-sm"
              value={formatFsType}
              onChange={(e) => setFormatFsType(e.target.value)}
            >
              <option value="f2fs">F2FS</option>
              <option value="ext4">Ext4</option>
            </select>
          </div>
          <Button
            variant="default"
            onClick={handleFormatData}
            disabled={formatLoading}
          >
            <Trash2 size={16} className="mr-2" />
            {formatLoading ? "Formatting..." : "Format Data"}
          </Button>
        </CardContent>
      </Card>

      <Separator />

      {/* Disable Vbmeta */}
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <ShieldOff size={20} />
            Disable AVB
          </CardTitle>
        </CardHeader>
        <CardContent>
          <p className="text-sm text-muted-foreground mb-3">
            Flash empty vbmeta to both slots to disable dm-verity and AVB
            verification.
          </p>
          <Button
            variant="destructive"
            onClick={handleDisableVbmeta}
            disabled={vbmetaLoading}
          >
            {vbmetaLoading ? "Working..." : "Disable Vbmeta"}
          </Button>
        </CardContent>
      </Card>
    </div>
  );
}
