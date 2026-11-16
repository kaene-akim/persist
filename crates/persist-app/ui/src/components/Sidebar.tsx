import { useStore, type Page } from "../store";
import { cx, StatusDot } from "./ui";
import {
  IconActivity,
  IconBackups,
  IconDashboard,
  IconRestore,
  IconSettings,
  IconSnapshots,
  IconStorage,
} from "./icons";

interface NavItem {
  id: Page;
  label: string;
  icon: (p: { className?: string }) => JSX.Element;
}

const NAV: NavItem[] = [
  { id: "dashboard", label: "Dashboard", icon: IconDashboard },
  { id: "backups", label: "My Backups", icon: IconBackups },
  { id: "snapshots", label: "Snapshots", icon: IconSnapshots },
  { id: "restore", label: "Restore", icon: IconRestore },
  { id: "activity", label: "Activity", icon: IconActivity },
  { id: "storage", label: "Storage", icon: IconStorage },
  { id: "settings", label: "Settings", icon: IconSettings },
];

export function Sidebar() {
  const { page, setPage, info, account, snapshots, roots } = useStore();

  const backendLabel = info?.backend === "local" ? "Local repository" : "Google Drive";

  return (
    <aside className="flex w-60 shrink-0 flex-col border-r border-line bg-panel">
      <div className="dither flex items-center gap-2.5 border-b border-line px-4 py-4">
        <div className="grid h-8 w-8 place-items-center border border-line bg-bg">
          <div className="h-3.5 w-3.5 bg-moss" />
        </div>
        <div className="leading-none">
          <div className="font-pixel text-sm tracking-tight text-ink">Persist</div>
          <div className="mt-1 text-[10px] text-faint">backup, versioned</div>
        </div>
      </div>

      <nav className="flex-1 overflow-y-auto p-2">
        {NAV.map((item) => {
          const active = page === item.id;
          const Icon = item.icon;
          return (
            <button
              key={item.id}
              onClick={() => setPage(item.id)}
              aria-current={active ? "page" : undefined}
              className={cx(
                "group mb-0.5 flex w-full items-center gap-3 px-2.5 py-2 text-sm transition-colors",
                active
                  ? "border-l-2 border-moss bg-panel2 text-ink"
                  : "border-l-2 border-transparent text-muted hover:bg-panel2 hover:text-ink",
              )}
            >
              <Icon className={cx("h-4 w-4", active ? "text-moss" : "text-faint group-hover:text-muted")} />
              <span className="font-medium">{item.label}</span>
            </button>
          );
        })}
      </nav>

      <div className="space-y-3 border-t border-line px-4 py-3.5 text-xs">
        <div className="flex items-center justify-between">
          <span className="text-faint">Storage</span>
          <span className="flex items-center gap-1.5 text-muted">
            <StatusDot tone={info?.initialised ? "moss" : "clay"} />
            {backendLabel}
          </span>
        </div>
        <div className="flex items-center justify-between">
          <span className="text-faint">Snapshots</span>
          <span className="font-mono text-muted">{snapshots.length}</span>
        </div>
        <div className="flex items-center justify-between">
          <span className="text-faint">Directories</span>
          <span className="font-mono text-muted">{roots.length}</span>
        </div>
        {info?.backend !== "local" && (
          <div className="flex items-center justify-between">
            <span className="text-faint">Account</span>
            <span className="max-w-[7.5rem] truncate text-muted" title={account?.email ?? ""}>
              {account ? account.email : "not connected"}
            </span>
          </div>
        )}
      </div>
    </aside>
  );
}
