import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { listen } from "@tauri-apps/api/event";
import { api, errorMessage } from "./lib/api";
import type {
  ActivityEntry,
  AppInfo,
  CachedSnapshot,
  ProgressEvent,
  RootConfig,
  Settings,
  StorageInfo,
  UserInfo,
} from "./lib/types";

export type Page =
  | "dashboard"
  | "backups"
  | "snapshots"
  | "restore"
  | "activity"
  | "storage"
  | "settings";

export interface Toast {
  id: number;
  kind: "success" | "error" | "info";
  message: string;
}

export interface ProgressState {
  phase: string;
  done: number;
  total: number;
  bytesDone: number;
  bytesTotal: number;
  message: string;
}

interface Store {
  info: AppInfo | null;
  roots: RootConfig[];
  snapshots: CachedSnapshot[];
  activity: ActivityEntry[];
  storage: StorageInfo | null;
  settings: Settings | null;
  account: UserInfo | null;

  page: Page;
  setPage: (p: Page) => void;
  restoreTarget: string | null;
  setRestoreTarget: (id: string | null) => void;

  bootstrapped: boolean;
  busy: string | null;
  progress: ProgressState | null;

  refreshAll: () => Promise<void>;
  refreshInfo: () => Promise<void>;
  refreshRoots: () => Promise<void>;
  refreshSnapshots: () => Promise<void>;
  refreshActivity: () => Promise<void>;
  refreshStorage: () => Promise<void>;
  refreshSettings: () => Promise<void>;
  refreshAccount: () => Promise<void>;

  run: <T>(label: string, fn: () => Promise<T>) => Promise<T | undefined>;
  cancel: () => void;

  toasts: Toast[];
  toast: (kind: Toast["kind"], message: string) => void;
  dismissToast: (id: number) => void;
}

const StoreContext = createContext<Store | null>(null);

export function useStore(): Store {
  const ctx = useContext(StoreContext);
  if (!ctx) throw new Error("useStore must be used inside <StoreProvider>");
  return ctx;
}

let toastSeq = 0;

export function StoreProvider({ children }: { children: ReactNode }) {
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [roots, setRoots] = useState<RootConfig[]>([]);
  const [snapshots, setSnapshots] = useState<CachedSnapshot[]>([]);
  const [activity, setActivity] = useState<ActivityEntry[]>([]);
  const [storage, setStorage] = useState<StorageInfo | null>(null);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [account, setAccount] = useState<UserInfo | null>(null);
  const [page, setPage] = useState<Page>("dashboard");
  const [restoreTarget, setRestoreTarget] = useState<string | null>(null);
  const [bootstrapped, setBootstrapped] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [progress, setProgress] = useState<ProgressState | null>(null);
  const [toasts, setToasts] = useState<Toast[]>([]);
  const busyRef = useRef(false);

  const dismissToast = useCallback((id: number) => {
    setToasts((t) => t.filter((x) => x.id !== id));
  }, []);

  const toast = useCallback(
    (kind: Toast["kind"], message: string) => {
      const id = ++toastSeq;
      setToasts((t) => [...t, { id, kind, message }]);
      window.setTimeout(() => dismissToast(id), kind === "error" ? 8000 : 4500);
    },
    [dismissToast],
  );

  const refreshInfo = useCallback(async () => {
    setInfo(await api.appInfo());
  }, []);
  const refreshRoots = useCallback(async () => {
    setRoots(await api.listRoots());
  }, []);
  const refreshSnapshots = useCallback(async () => {
    setSnapshots(await api.listSnapshots(100));
  }, []);
  const refreshActivity = useCallback(async () => {
    setActivity(await api.listActivity(100));
  }, []);
  const refreshStorage = useCallback(async () => {
    try {
      setStorage(await api.storageInfo());
    } catch {
      setStorage(null);
    }
  }, []);
  const refreshSettings = useCallback(async () => {
    setSettings(await api.getSettings());
  }, []);
  const refreshAccount = useCallback(async () => {
    try {
      setAccount(await api.account());
    } catch {
      setAccount(null);
    }
  }, []);

  const refreshAll = useCallback(async () => {
    await refreshInfo();
    await Promise.allSettled([
      refreshRoots(),
      refreshSnapshots(),
      refreshActivity(),
      refreshSettings(),
      refreshAccount(),
    ]);
  }, [refreshInfo, refreshRoots, refreshSnapshots, refreshActivity, refreshSettings, refreshAccount]);

  // Initial load.
  useEffect(() => {
    (async () => {
      try {
        await refreshAll();
        await refreshStorage();
      } catch (e) {
        toast("error", errorMessage(e));
      } finally {
        setBootstrapped(true);
      }
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const run = useCallback(
    async <T,>(label: string, fn: () => Promise<T>): Promise<T | undefined> => {
      if (busyRef.current) {
        toast("info", "Another operation is already running.");
        return undefined;
      }
      busyRef.current = true;
      setBusy(label);
      setProgress({ phase: label, done: 0, total: 0, bytesDone: 0, bytesTotal: 0, message: "" });
      const unlisten = await listen<ProgressEvent>("persist://progress", (event) => {
        const p = event.payload;
        setProgress((prev) => {
          const base: ProgressState =
            prev ?? { phase: label, done: 0, total: 0, bytesDone: 0, bytesTotal: 0, message: "" };
          if (p.kind === "phase") return { ...base, phase: p.phase, total: p.total, done: 0 };
          if (p.kind === "progress")
            return {
              ...base,
              done: p.done,
              total: p.total,
              bytesDone: p.bytes_done,
              bytesTotal: p.bytes_total,
            };
          return { ...base, message: p.message };
        });
      });
      try {
        return await fn();
      } catch (e) {
        toast("error", errorMessage(e));
        return undefined;
      } finally {
        unlisten();
        busyRef.current = false;
        setBusy(null);
        setProgress(null);
      }
    },
    [toast],
  );

  const cancel = useCallback(() => {
    void api.cancelOperation();
  }, []);

  const value = useMemo<Store>(
    () => ({
      info,
      roots,
      snapshots,
      activity,
      storage,
      settings,
      account,
      page,
      setPage,
      restoreTarget,
      setRestoreTarget,
      bootstrapped,
      busy,
      progress,
      refreshAll,
      refreshInfo,
      refreshRoots,
      refreshSnapshots,
      refreshActivity,
      refreshStorage,
      refreshSettings,
      refreshAccount,
      run,
      cancel,
      toasts,
      toast,
      dismissToast,
    }),
    [
      info,
      roots,
      snapshots,
      activity,
      storage,
      settings,
      account,
      page,
      bootstrapped,
      busy,
      progress,
      restoreTarget,
      refreshAll,
      refreshInfo,
      refreshRoots,
      refreshSnapshots,
      refreshActivity,
      refreshStorage,
      refreshSettings,
      refreshAccount,
      run,
      cancel,
      toasts,
      toast,
      dismissToast,
    ],
  );

  return <StoreContext.Provider value={value}>{children}</StoreContext.Provider>;
}
