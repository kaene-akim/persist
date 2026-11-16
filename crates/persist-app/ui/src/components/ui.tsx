import type { ButtonHTMLAttributes, ReactNode } from "react";

export function cx(...parts: Array<string | false | null | undefined>): string {
  return parts.filter(Boolean).join(" ");
}

// -- Button -----------------------------------------------------------------

type Variant = "primary" | "secondary" | "ghost" | "danger";

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: Variant;
  size?: "sm" | "md";
  loading?: boolean;
}

const VARIANTS: Record<Variant, string> = {
  primary:
    "bg-moss text-bg hover:bg-mossdeep shadow-pixel active:translate-x-[1px] active:translate-y-[1px] active:shadow-none",
  secondary:
    "bg-panel2 text-ink border border-line hover:border-line2 active:translate-y-[1px]",
  ghost: "text-muted hover:text-ink hover:bg-panel2",
  danger: "bg-rust text-ink hover:brightness-110",
};

export function Button({
  variant = "secondary",
  size = "md",
  loading,
  className,
  children,
  disabled,
  ...rest
}: ButtonProps) {
  return (
    <button
      {...rest}
      disabled={disabled || loading}
      className={cx(
        "inline-flex items-center justify-center gap-2 font-medium tracking-tight",
        "disabled:opacity-45 disabled:pointer-events-none select-none transition-colors",
        size === "sm" ? "px-2.5 py-1.5 text-xs" : "px-3.5 py-2 text-sm",
        VARIANTS[variant],
        className,
      )}
    >
      {loading && <Spinner className="h-3.5 w-3.5" />}
      {children}
    </button>
  );
}

// -- Surfaces ---------------------------------------------------------------

export function Card({
  children,
  className,
  as: As = "div",
}: {
  children: ReactNode;
  className?: string;
  as?: "div" | "section" | "li";
}) {
  return <As className={cx("bg-panel border border-line", className)}>{children}</As>;
}

export function CardHeader({
  title,
  hint,
  action,
}: {
  title: ReactNode;
  hint?: ReactNode;
  action?: ReactNode;
}) {
  return (
    <div className="flex items-start justify-between gap-4 px-4 py-3 border-b border-line">
      <div className="min-w-0">
        <h3 className="text-sm font-semibold text-ink">{title}</h3>
        {hint && <p className="mt-0.5 text-xs text-muted">{hint}</p>}
      </div>
      {action}
    </div>
  );
}

// -- Badge ------------------------------------------------------------------

type Tone = "moss" | "clay" | "water" | "rust" | "neutral";

const TONES: Record<Tone, string> = {
  moss: "text-moss border-moss/40 bg-moss/10",
  clay: "text-clay border-clay/40 bg-clay/10",
  water: "text-water border-water/40 bg-water/10",
  rust: "text-rust border-rust/40 bg-rust/10",
  neutral: "text-muted border-line bg-panel2",
};

export function Badge({ tone = "neutral", children }: { tone?: Tone; children: ReactNode }) {
  return (
    <span
      className={cx(
        "inline-flex items-center gap-1 border px-1.5 py-0.5 text-[11px] font-medium",
        TONES[tone],
      )}
    >
      {children}
    </span>
  );
}

/** A square status glyph, echoing the pixel-art influence. */
export function StatusDot({ tone = "neutral" }: { tone?: Tone }) {
  const colors: Record<Tone, string> = {
    moss: "bg-moss",
    clay: "bg-clay",
    water: "bg-water",
    rust: "bg-rust",
    neutral: "bg-faint",
  };
  return <span className={cx("inline-block h-2 w-2", colors[tone])} aria-hidden />;
}

// -- Stat -------------------------------------------------------------------

export function Stat({
  label,
  value,
  sub,
  tone = "neutral",
}: {
  label: string;
  value: ReactNode;
  sub?: ReactNode;
  tone?: Tone;
}) {
  const accent: Record<Tone, string> = {
    moss: "text-moss",
    clay: "text-clay",
    water: "text-water",
    rust: "text-rust",
    neutral: "text-ink",
  };
  return (
    <div className="px-4 py-3.5">
      <div className="text-xs text-muted">{label}</div>
      <div className={cx("mt-1 font-pixel text-lg leading-tight", accent[tone])}>{value}</div>
      {sub && <div className="mt-1 text-xs text-faint">{sub}</div>}
    </div>
  );
}

// -- Empty state ------------------------------------------------------------

export function EmptyState({
  title,
  children,
  action,
}: {
  title: string;
  children?: ReactNode;
  action?: ReactNode;
}) {
  return (
    <div className="dither flex flex-col items-center justify-center gap-3 border border-dashed border-line px-6 py-12 text-center">
      <h3 className="text-sm font-semibold text-ink">{title}</h3>
      {children && <p className="max-w-md text-xs leading-relaxed text-muted">{children}</p>}
      {action}
    </div>
  );
}

// -- Spinner / progress -----------------------------------------------------

export function Spinner({ className }: { className?: string }) {
  return (
    <svg className={cx("animate-spin", className)} viewBox="0 0 24 24" fill="none" aria-hidden>
      <circle cx="12" cy="12" r="9" stroke="currentColor" strokeOpacity="0.25" strokeWidth="3" />
      <path d="M21 12a9 9 0 0 0-9-9" stroke="currentColor" strokeWidth="3" />
    </svg>
  );
}

export function ProgressBar({ value, max }: { value: number; max: number }) {
  const pct = max > 0 ? Math.min(100, (value / max) * 100) : 0;
  return (
    <div className="h-2 w-full border border-line bg-bg">
      <div
        className="h-full bg-moss transition-[width] duration-200"
        style={{ width: `${pct}%` }}
      />
    </div>
  );
}

// -- Form fields ------------------------------------------------------------

export function Field({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: ReactNode;
  children: ReactNode;
}) {
  return (
    <label className="block">
      <span className="label">{label}</span>
      <div className="mt-1.5">{children}</div>
      {hint && <p className="mt-1.5 text-xs text-faint">{hint}</p>}
    </label>
  );
}

export function Toggle({
  checked,
  onChange,
  label,
  hint,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  label: string;
  hint?: string;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      onClick={() => onChange(!checked)}
      className="flex w-full items-center justify-between gap-4 py-2 text-left"
    >
      <span className="min-w-0">
        <span className="block text-sm text-ink">{label}</span>
        {hint && <span className="block text-xs text-faint">{hint}</span>}
      </span>
      <span
        className={cx(
          "relative inline-flex h-5 w-9 shrink-0 items-center border transition-colors",
          checked ? "border-moss bg-moss/30" : "border-line bg-bg",
        )}
      >
        <span
          className={cx(
            "absolute h-3 w-3 transition-all",
            checked ? "left-[18px] bg-moss" : "left-1 bg-faint",
          )}
        />
      </span>
    </button>
  );
}

// -- Modal ------------------------------------------------------------------

export function Modal({
  title,
  onClose,
  children,
  footer,
}: {
  title: string;
  onClose: () => void;
  children: ReactNode;
  footer?: ReactNode;
}) {
  return (
    <div
      className="fixed inset-0 z-40 flex items-center justify-center bg-black/60 p-6 animate-fade-in"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="w-full max-w-lg border border-line bg-panel shadow-pixel">
        <div className="flex items-center justify-between border-b border-line px-4 py-3">
          <h2 className="text-sm font-semibold">{title}</h2>
          <button
            onClick={onClose}
            className="px-1.5 text-muted hover:text-ink"
            aria-label="Close"
          >
            ✕
          </button>
        </div>
        <div className="px-4 py-4">{children}</div>
        {footer && (
          <div className="flex justify-end gap-2 border-t border-line px-4 py-3">{footer}</div>
        )}
      </div>
    </div>
  );
}
