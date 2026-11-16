// Small, blocky line icons drawn on a 24x24 grid. Kept as components so they
// inherit currentColor and stay crisp at small sizes.

type IconProps = { className?: string };

function svgProps(className?: string) {
  return {
    className: className ?? "h-4 w-4",
    viewBox: "0 0 24 24",
    fill: "none",
    stroke: "currentColor",
    strokeWidth: 1.75,
    strokeLinecap: "square" as const,
    strokeLinejoin: "miter" as const,
    "aria-hidden": true,
  };
}

export const IconDashboard = ({ className }: IconProps) => (
  <svg {...svgProps(className)}>
    <rect x="3" y="3" width="7" height="7" />
    <rect x="14" y="3" width="7" height="7" />
    <rect x="3" y="14" width="7" height="7" />
    <rect x="14" y="14" width="7" height="7" />
  </svg>
);

export const IconBackups = ({ className }: IconProps) => (
  <svg {...svgProps(className)}>
    <path d="M3 7l2-3h14l2 3" />
    <path d="M4 7h16v13H4z" />
    <path d="M3 7h18" />
    <path d="M10 11h4" />
  </svg>
);

export const IconSnapshots = ({ className }: IconProps) => (
  <svg {...svgProps(className)}>
    <ellipse cx="12" cy="6" rx="7" ry="3" />
    <path d="M5 6v6c0 1.66 3.13 3 7 3s7-1.34 7-3V6" />
    <path d="M5 12v6c0 1.66 3.13 3 7 3s7-1.34 7-3v-6" />
  </svg>
);

export const IconRestore = ({ className }: IconProps) => (
  <svg {...svgProps(className)}>
    <path d="M12 3v11" />
    <path d="M8 10l4 4 4-4" />
    <path d="M4 17v3h16v-3" />
  </svg>
);

export const IconActivity = ({ className }: IconProps) => (
  <svg {...svgProps(className)}>
    <path d="M3 12h4l2-6 3 12 2-6h7" />
  </svg>
);

export const IconStorage = ({ className }: IconProps) => (
  <svg {...svgProps(className)}>
    <rect x="3" y="4" width="18" height="6" />
    <rect x="3" y="14" width="18" height="6" />
    <path d="M7 7h.01M7 17h.01" />
  </svg>
);

export const IconSettings = ({ className }: IconProps) => (
  <svg {...svgProps(className)}>
    <path d="M4 6h10M18 6h2" />
    <path d="M4 12h4M12 12h8" />
    <path d="M4 18h8M16 18h4" />
    <rect x="14" y="4" width="4" height="4" />
    <rect x="8" y="10" width="4" height="4" />
    <rect x="12" y="16" width="4" height="4" />
  </svg>
);

export const IconPlus = ({ className }: IconProps) => (
  <svg {...svgProps(className)}>
    <path d="M12 5v14M5 12h14" />
  </svg>
);

export const IconCheck = ({ className }: IconProps) => (
  <svg {...svgProps(className)}>
    <path d="M4 12l5 5L20 6" />
  </svg>
);

export const IconAlert = ({ className }: IconProps) => (
  <svg {...svgProps(className)}>
    <path d="M12 3l9 16H3z" />
    <path d="M12 9v5M12 17h.01" />
  </svg>
);

export const IconRefresh = ({ className }: IconProps) => (
  <svg {...svgProps(className)}>
    <path d="M20 11a8 8 0 1 0-2.3 5.7" />
    <path d="M20 5v6h-6" />
  </svg>
);

export const IconClose = ({ className }: IconProps) => (
  <svg {...svgProps(className)}>
    <path d="M6 6l12 12M18 6L6 18" />
  </svg>
);

export const IconGoogle = ({ className }: IconProps) => (
  <svg className={className ?? "h-4 w-4"} viewBox="0 0 24 24" aria-hidden fill="currentColor">
    <path d="M21.35 11.1H12v2.9h5.35c-.5 2.4-2.55 3.8-5.35 3.8A6.2 6.2 0 0 1 5.8 11.6 6.2 6.2 0 0 1 12 5.4c1.5 0 2.8.55 3.85 1.45l2.1-2.1A9.1 9.1 0 0 0 12 2 10 10 0 0 0 2 12a10 10 0 0 0 10 10c5.75 0 9.6-4 9.6-9.65 0-.65-.1-1.2-.25-1.25z" />
  </svg>
);
