import { useStore, type Page } from "./store";
import { Sidebar } from "./components/Sidebar";
import { OperationBar, TopBar } from "./components/TopBar";
import { Toasts } from "./components/Toasts";
import { Spinner } from "./components/ui";
import { Dashboard } from "./pages/Dashboard";
import { Backups } from "./pages/Backups";
import { Snapshots } from "./pages/Snapshots";
import { Restore } from "./pages/Restore";
import { Activity } from "./pages/Activity";
import { Storage } from "./pages/Storage";
import { Settings } from "./pages/Settings";
import { Setup } from "./pages/Setup";

const TITLES: Record<Page, { title: string; subtitle?: string }> = {
  dashboard: {
    title: "Dashboard",
    subtitle: "The state of your backups at a glance.",
  },
  backups: {
    title: "My Backups",
    subtitle: "The directories Persist protects on this machine.",
  },
  snapshots: { title: "Snapshots", subtitle: "Every version, kept forever." },
  restore: {
    title: "Restore",
    subtitle: "Bring back any file or directory from any snapshot.",
  },
  activity: { title: "Activity", subtitle: "What Persist has done, and when." },
  storage: {
    title: "Storage",
    subtitle: "What the repository holds and how much space it uses.",
  },
  settings: { title: "Settings", subtitle: "Account, storage and backup behaviour." },
};

function CurrentPage({ page }: { page: Page }) {
  switch (page) {
    case "dashboard":
      return <Dashboard />;
    case "backups":
      return <Backups />;
    case "snapshots":
      return <Snapshots />;
    case "restore":
      return <Restore />;
    case "activity":
      return <Activity />;
    case "storage":
      return <Storage />;
    case "settings":
      return <Settings />;
  }
}

export function App() {
  const { bootstrapped, info, page } = useStore();

  if (!bootstrapped) {
    return (
      <div className="grid h-full place-items-center">
        <div className="flex items-center gap-3 text-sm text-muted">
          <Spinner className="h-4 w-4 text-moss" />
          Starting Persist…
        </div>
      </div>
    );
  }

  // Before a repository exists, the whole window is the setup flow.
  if (!info?.initialised) {
    return (
      <>
        <Setup />
        <Toasts />
      </>
    );
  }

  const { title, subtitle } = TITLES[page];

  return (
    <div className="flex h-full">
      <Sidebar />
      <div className="flex min-w-0 flex-1 flex-col">
        <TopBar title={title} subtitle={subtitle} />
        <OperationBar />
        <main className="min-h-0 flex-1 overflow-y-auto bg-bg">
          <CurrentPage page={page} />
        </main>
      </div>
      <Toasts />
    </div>
  );
}
