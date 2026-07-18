export default function SettingsTab() {
  return (
    <div className="max-w-sm max-sm:max-w-full space-y-3">
      <div className="panel-shell px-5 py-4 space-y-2">
        <InfoRow label="Name" value="pawflash" />
        <InfoRow label="Version" value="0.1.0" />
        <InfoRow label="Stack" value="Tauri v2 + React 19 + Rust" />
        <InfoRow label="License" value="GPL-3.0-or-later" />
      </div>
      <p className="text-caption text-muted-foreground/60 px-1 font-mono">
        · mtk device flashing toolkit
      </p>
    </div>
  );
}

function InfoRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-baseline justify-between gap-4">
      <span className="text-caption font-display font-medium uppercase tracking-wider text-muted-foreground/70">
        {label}
      </span>
      <span className="text-body text-foreground/90 text-right">{value}</span>
    </div>
  );
}
