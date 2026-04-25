import { NavLink, useNavigate } from "react-router-dom";
import { useState, type ReactNode } from "react";

import { logout } from "../lib/auth";

// Sidebar entries are declared as data so adding a new panel is a one-line
// change rather than copy/pasting JSX. `enabled: false` rows render as
// disabled placeholders until their page lands.
interface NavEntry {
  label: string;
  to: string;
  enabled: boolean;
  children?: NavEntry[];
}

const NAV_ENTRIES: NavEntry[] = [
  { label: "Dashboard", to: "/v2/dashboard", enabled: true },
  { label: "Chart", to: "/v2/chart", enabled: true },
  // FAZ 25 PR-25A — dedicated demo page for the IQ-D / IQ-T predictive
  // layer. Currently shows the Elliott early-wave (N/F/X) markers; will
  // grow to host IQ structure tracker visualisation as later PRs land.
  { label: "IQ Chart", to: "/v2/iq-chart", enabled: true },
  { label: "RADAR Reports", to: "/v2/reports", enabled: true },
  { label: "Setups", to: "/v2/setups", enabled: true },
  { label: "Detections", to: "/v2/detections", enabled: true },
  { label: "Regime", to: "/v2/regime", enabled: true },
  { label: "Config", to: "/v2/config", enabled: true },
  { label: "Users", to: "/v2/users", enabled: true },
  { label: "Engine Symbols", to: "/v2/engine-symbols", enabled: true },
  { label: "Symbols (Intel)", to: "/v2/symbols", enabled: true },
];

// One sidebar row. Parents with `children` render as a collapsible group;
// leaves render as NavLinks. Kept flat (no recursion beyond one level) —
// all we need for now is "Rapor" > "Backtest".
function NavEntryRow({
  entry,
  onNavigate,
}: {
  entry: NavEntry;
  onNavigate: () => void;
}) {
  const [open, setOpen] = useState(true);
  if (!entry.enabled) {
    return (
      <span
        className="cursor-not-allowed rounded px-3 py-2 text-sm text-zinc-600"
        title="coming soon"
      >
        {entry.label}
      </span>
    );
  }
  if (entry.children && entry.children.length > 0) {
    return (
      <div className="flex flex-col gap-0.5">
        <button
          type="button"
          onClick={() => setOpen((o) => !o)}
          className="flex items-center justify-between rounded px-3 py-2 text-sm text-zinc-300 hover:bg-zinc-800/70 hover:text-zinc-100"
        >
          <span>{entry.label}</span>
          <span className="text-zinc-500">{open ? "−" : "+"}</span>
        </button>
        {open &&
          entry.children.map((c) => (
            <NavLink
              key={c.to}
              to={c.to}
              onClick={onNavigate}
              className={({ isActive }) =>
                `ml-4 rounded px-3 py-1.5 text-sm transition ${
                  isActive
                    ? "bg-emerald-500/15 text-emerald-300"
                    : "text-zinc-400 hover:bg-zinc-800/70 hover:text-zinc-100"
                }`
              }
            >
              {c.label}
            </NavLink>
          ))}
      </div>
    );
  }
  return (
    <NavLink
      to={entry.to}
      onClick={onNavigate}
      className={({ isActive }) =>
        `rounded px-3 py-2 text-sm transition ${
          isActive
            ? "bg-emerald-500/15 text-emerald-300"
            : "text-zinc-300 hover:bg-zinc-800/70 hover:text-zinc-100"
        }`
      }
    >
      {entry.label}
    </NavLink>
  );
}

export function Layout({ children }: { children: ReactNode }) {
  const navigate = useNavigate();
  const [navOpen, setNavOpen] = useState(false);
  const handleLogout = () => {
    logout();
    navigate("/login", { replace: true });
  };

  return (
    <div className="relative flex min-h-screen">
      {navOpen && (
        <div
          className="fixed inset-0 z-30 bg-black/50"
          onClick={() => setNavOpen(false)}
        />
      )}
      <aside
        className={`fixed inset-y-0 left-0 z-40 w-56 shrink-0 transform border-r border-zinc-800 bg-zinc-900 transition-transform duration-200 ${
          navOpen ? "translate-x-0" : "-translate-x-full"
        }`}
      >
        <div className="px-4 py-5 text-lg font-semibold tracking-tight text-zinc-100">
          QTSS <span className="text-emerald-400">v2</span>
        </div>
        <nav className="flex flex-col gap-1 px-2 pb-4">
          {NAV_ENTRIES.map((entry) => (
            <NavEntryRow
              key={entry.to}
              entry={entry}
              onNavigate={() => setNavOpen(false)}
            />
          ))}
        </nav>
      </aside>
      <div className="flex min-h-screen flex-1 flex-col">
        <header className="flex items-center justify-between border-b border-zinc-800 bg-zinc-900/40 px-4 py-3">
          <div className="flex items-center gap-3">
            <button
              type="button"
              onClick={() => setNavOpen((o) => !o)}
              aria-label="Toggle navigation"
              className="flex h-8 w-8 items-center justify-center rounded border border-zinc-700 text-zinc-300 hover:border-zinc-500 hover:text-zinc-100"
            >
              <svg
                xmlns="http://www.w3.org/2000/svg"
                width="16"
                height="16"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="2"
                strokeLinecap="round"
                strokeLinejoin="round"
              >
                <line x1="3" y1="6" x2="21" y2="6" />
                <line x1="3" y1="12" x2="21" y2="12" />
                <line x1="3" y1="18" x2="21" y2="18" />
              </svg>
            </button>
            <div className="text-sm text-zinc-400">
              QTSS <span className="text-emerald-400">v2</span> console
            </div>
          </div>
          <button
            type="button"
            onClick={handleLogout}
            className="rounded border border-zinc-700 px-3 py-1 text-xs text-zinc-300 hover:border-zinc-500 hover:text-zinc-100"
          >
            Logout
          </button>
        </header>
        <main className="flex-1 overflow-auto p-6">{children}</main>
      </div>
    </div>
  );
}
