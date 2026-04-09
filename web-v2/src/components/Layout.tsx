import { NavLink, useNavigate } from "react-router-dom";
import type { ReactNode } from "react";

import { logout } from "../lib/auth";

// Sidebar entries are declared as data so adding a new panel is a one-line
// change rather than copy/pasting JSX. `enabled: false` rows render as
// disabled placeholders until their page lands.
interface NavEntry {
  label: string;
  to: string;
  enabled: boolean;
}

const NAV_ENTRIES: NavEntry[] = [
  { label: "Dashboard", to: "/v2/dashboard", enabled: true },
  { label: "Chart", to: "/v2/chart", enabled: false },
  { label: "Regime", to: "/v2/regime", enabled: false },
  { label: "Scenarios", to: "/v2/scenarios", enabled: false },
  { label: "Monte Carlo", to: "/v2/montecarlo", enabled: false },
  { label: "Risk", to: "/v2/risk", enabled: true },
  { label: "Blotter", to: "/v2/blotter", enabled: false },
  { label: "Strategies", to: "/v2/strategies", enabled: false },
  { label: "Config", to: "/v2/config", enabled: false },
  { label: "AI Decisions", to: "/v2/ai-decisions", enabled: false },
  { label: "Audit", to: "/v2/audit", enabled: false },
  { label: "Users", to: "/v2/users", enabled: false },
];

export function Layout({ children }: { children: ReactNode }) {
  const navigate = useNavigate();
  const handleLogout = () => {
    logout();
    navigate("/login", { replace: true });
  };

  return (
    <div className="flex min-h-screen">
      <aside className="w-56 shrink-0 border-r border-zinc-800 bg-zinc-900/60">
        <div className="px-4 py-5 text-lg font-semibold tracking-tight text-zinc-100">
          QTSS <span className="text-emerald-400">v2</span>
        </div>
        <nav className="flex flex-col gap-1 px-2 pb-4">
          {NAV_ENTRIES.map((entry) =>
            entry.enabled ? (
              <NavLink
                key={entry.to}
                to={entry.to}
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
            ) : (
              <span
                key={entry.to}
                className="cursor-not-allowed rounded px-3 py-2 text-sm text-zinc-600"
                title="coming soon"
              >
                {entry.label}
              </span>
            ),
          )}
        </nav>
      </aside>
      <div className="flex min-h-screen flex-1 flex-col">
        <header className="flex items-center justify-between border-b border-zinc-800 bg-zinc-900/40 px-6 py-3">
          <div className="text-sm text-zinc-400">QTSS v2 console</div>
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
