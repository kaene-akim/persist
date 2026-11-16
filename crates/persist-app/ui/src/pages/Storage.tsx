import { useState } from "react";
import { api } from "../lib/api";
import { useStore } from "../store";
import {
  Badge,
  Button,
  Card,
  CardHeader,
  EmptyState,
  ProgressBar,
  Stat,
  StatusDot,
  cx,
} from "../components/ui";
import { humanBytes, humanCount, percent } from "../lib/format";
import type { VerifyReport } from "../lib/types";

export function Storage() {
  const { storage, refreshStorage, run, toast, info } = useStore();
  const [report, setReport] = useState<VerifyReport | null>(null);

  const usage = storage?.usage;
  const quota = storage?.quota;

  const refresh = () =>
    run("Refreshing storage", async () => {
      await refreshStorage();
    });

  const verify = (deep: boolean) =>
    run(deep ? "Verifying every object" : "Verifying repository", async () => {
      const r = await api.verify(deep);
      setReport(r);
      if (r.ok) {
        toast("success", "Verification passed — every referenced object is present and intact.");
      } else {
        toast(
          "error",
          `Verification found problems: ${r.missing_objects.length} missing, ${r.corrupt_objects.length} corrupt.`,
        );
      }
    });

  const dedupRatio =
    usage && usage.logical_bytes > 0
      ? usage.logical_bytes / Math.max(1, usage.physical_bytes)
      : 0;

  return (
    <div className="space-y-6 px-6 py-6">
      <div className="grid grid-cols-2 gap-4 lg:grid-cols-4">
        <Card>
          <Stat
            label="Stored (physical)"
            value={usage ? humanBytes(usage.physical_bytes) : "—"}
            sub={usage ? `${humanCount(usage.object_count)} objects` : undefined}
            tone="moss"
          />
        </Card>
        <Card>
          <Stat
            label="Logical data"
            value={usage ? humanBytes(usage.logical_bytes) : "—"}
            sub={usage ? `${humanCount(usage.snapshot_count)} snapshots` : undefined}
          />
        </Card>
        <Card>
          <Stat
            label="Deduplication"
            value={dedupRatio > 0 ? `${dedupRatio.toFixed(2)}×` : "—"}
            sub="logical vs. stored"
            tone="water"
          />
        </Card>
        <Card>
          <Stat
            label="Backend"
            value={info?.backend === "local" ? "Local" : "Google Drive"}
            sub={storage?.backend}
          />
        </Card>
      </div>

      <div className="grid grid-cols-1 gap-6 lg:grid-cols-2">
        <Card>
          <CardHeader
            title="Repository usage"
            hint="Objects are stored once and shared by every snapshot that references them."
            action={
              <Button variant="secondary" size="sm" onClick={refresh}>
                Refresh
              </Button>
            }
          />
          {usage ? (
            <dl className="divide-y divide-line">
              <Row label="Content objects" value={humanCount(usage.object_count)} />
              <Row label="Stored bytes" value={humanBytes(usage.physical_bytes)} />
              <Row label="Snapshots" value={humanCount(usage.snapshot_count)} />
              <Row
                label="Total across snapshots"
                value={humanBytes(usage.total_logical_bytes)}
              />
            </dl>
          ) : (
            <div className="p-4">
              <EmptyState title="No usage information">
                Initialise a repository, or refresh to read the current usage.
              </EmptyState>
            </div>
          )}
        </Card>

        <Card>
          <CardHeader
            title="Provider quota"
            hint={
              quota
                ? "Reported by the storage backend."
                : "Available when backing up to Google Drive."
            }
          />
          {quota ? (
            <div className="space-y-4 px-4 py-4">
              <div>
                <div className="mb-2 flex items-baseline justify-between text-sm">
                  <span className="text-muted">
                    {humanBytes(quota.usage)} of {humanBytes(quota.limit)}
                  </span>
                  <span className="font-mono text-xs text-faint">
                    {percent(quota.usage, quota.limit).toFixed(1)}%
                  </span>
                </div>
                <ProgressBar value={quota.usage} max={quota.limit} />
              </div>
              <dl className="divide-y divide-line border-t border-line">
                <Row label="Used in Drive" value={humanBytes(quota.usage_in_drive)} />
                <Row label="In Drive trash" value={humanBytes(quota.usage_in_drive_trash)} />
              </dl>
            </div>
          ) : (
            <div className="px-4 py-4">
              <p className="text-xs leading-relaxed text-faint">
                The local backend has no quota. Google Drive reports the storage limit of the
                connected account.
              </p>
            </div>
          )}
        </Card>
      </div>

      <Card>
        <CardHeader
          title="Integrity check"
          hint="Proves that every object a snapshot references can still be read and hashes correctly."
          action={
            <div className="flex gap-2">
              <Button variant="secondary" size="sm" onClick={() => verify(false)}>
                Quick check
              </Button>
              <Button variant="primary" size="sm" onClick={() => verify(true)}>
                Deep verify
              </Button>
            </div>
          }
        />
        {report ? (
          <div className="space-y-4 px-4 py-4">
            <div className="flex items-center gap-2">
              <StatusDot tone={report.ok ? "moss" : "rust"} />
              <span className="text-sm text-ink">
                {report.ok ? "Repository is healthy" : "Problems were found"}
              </span>
              <Badge tone={report.ok ? "moss" : "rust"}>
                {report.objects_checked} of {report.objects_referenced} objects checked
              </Badge>
            </div>
            <div className="grid grid-cols-2 gap-x-6 gap-y-3 sm:grid-cols-4">
              <Metric label="Snapshots" value={humanCount(report.snapshots_checked)} />
              <Metric
                label="Missing"
                value={humanCount(report.missing_objects.length)}
                bad={report.missing_objects.length > 0}
              />
              <Metric
                label="Corrupt"
                value={humanCount(report.corrupt_objects.length)}
                bad={report.corrupt_objects.length > 0}
              />
              <Metric
                label="Snapshot issues"
                value={humanCount(report.snapshot_issues.length)}
                bad={report.snapshot_issues.length > 0}
              />
            </div>
            {report.missing_objects.length > 0 && (
              <IssueList title="Missing objects" items={report.missing_objects} />
            )}
            {report.corrupt_objects.length > 0 && (
              <IssueList
                title="Corrupt objects"
                items={report.corrupt_objects.map((c) => c.hash)}
              />
            )}
            {report.snapshot_issues.length > 0 && (
              <IssueList title="Snapshot issues" items={report.snapshot_issues} />
            )}
          </div>
        ) : (
          <div className="px-4 py-4">
            <p className="text-xs leading-relaxed text-faint">
              A quick check confirms every referenced object exists. A deep verify downloads and
              re-hashes each one — thorough, but it reads the whole repository.
            </p>
          </div>
        )}
      </Card>
    </div>
  );
}

function Row({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-center justify-between px-4 py-3">
      <dt className="text-sm text-muted">{label}</dt>
      <dd className="font-mono text-xs text-ink">{value}</dd>
    </div>
  );
}

function Metric({ label, value, bad }: { label: string; value: string; bad?: boolean }) {
  return (
    <div>
      <div className="text-xs text-faint">{label}</div>
      <div className={cx("mt-0.5 text-sm", bad ? "text-rust" : "text-ink")}>{value}</div>
    </div>
  );
}

function IssueList({ title, items }: { title: string; items: string[] }) {
  return (
    <div>
      <div className="mb-1.5 text-xs font-medium text-muted">{title}</div>
      <ul className="max-h-40 divide-y divide-line overflow-y-auto border border-line">
        {items.slice(0, 200).map((item, i) => (
          <li key={`${item}:${i}`} className="truncate px-3 py-1.5 font-mono text-[11px] text-muted">
            {item}
          </li>
        ))}
      </ul>
    </div>
  );
}
