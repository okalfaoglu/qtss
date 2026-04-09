import { useQuery } from "@tanstack/react-query";

import { apiFetch } from "../lib/api";
import type { UsersView } from "../lib/types";

function Tag({ children, tone }: { children: string; tone: "role" | "perm" | "admin" }) {
  // Three tones, kept in a single map so adding a new tag category is
  // a one-line change here.
  const cls = {
    role: "border-sky-500/30 bg-sky-500/10 text-sky-300",
    perm: "border-zinc-700 bg-zinc-800/60 text-zinc-300",
    admin: "border-emerald-500/40 bg-emerald-500/15 text-emerald-300",
  }[tone];
  return (
    <span className={`mr-1 inline-block rounded border px-1.5 py-0.5 text-[10px] font-mono ${cls}`}>
      {children}
    </span>
  );
}

export function Users() {
  const query = useQuery({
    queryKey: ["v2", "users"],
    queryFn: () => apiFetch<UsersView>("/v2/users"),
    refetchInterval: 30_000,
  });

  if (query.isLoading) {
    return <div className="text-sm text-zinc-400">Loading users…</div>;
  }
  if (query.isError) {
    return (
      <div className="rounded border border-red-800 bg-red-950/40 p-4 text-sm text-red-300">
        Failed: {(query.error as Error).message}
      </div>
    );
  }
  const view = query.data!;

  return (
    <div className="space-y-3">
      <div className="flex items-baseline justify-between">
        <div className="text-xs uppercase tracking-wide text-zinc-500">
          Users — {view.users.length}
        </div>
        <div className="text-xs text-zinc-500">Generated at {view.generated_at}</div>
      </div>

      <div className="overflow-hidden rounded-lg border border-zinc-800 bg-zinc-900/60">
        {view.users.length === 0 ? (
          <div className="px-4 py-6 text-sm text-zinc-500">No users in this org.</div>
        ) : (
          <table className="w-full text-sm">
            <thead className="bg-zinc-900/80 text-xs uppercase text-zinc-500">
              <tr>
                <th className="px-3 py-2 text-left">Email</th>
                <th className="px-3 py-2 text-left">Name</th>
                <th className="px-3 py-2 text-left">Roles</th>
                <th className="px-3 py-2 text-left">Permissions</th>
                <th className="px-3 py-2 text-left">Created</th>
              </tr>
            </thead>
            <tbody>
              {view.users.map((u) => (
                <tr key={u.id} className="border-t border-zinc-800/60 align-top">
                  <td className="px-3 py-2 font-mono text-zinc-100">
                    {u.email}
                    {u.is_admin && (
                      <span className="ml-2">
                        <Tag tone="admin">admin</Tag>
                      </span>
                    )}
                  </td>
                  <td className="px-3 py-2 text-zinc-300">{u.display_name ?? "—"}</td>
                  <td className="px-3 py-2">
                    {u.roles.length === 0 ? (
                      <span className="text-zinc-500">—</span>
                    ) : (
                      u.roles.map((r) => (
                        <Tag key={r} tone="role">
                          {r}
                        </Tag>
                      ))
                    )}
                  </td>
                  <td className="px-3 py-2">
                    {u.permissions.length === 0 ? (
                      <span className="text-zinc-500">—</span>
                    ) : (
                      u.permissions.map((p) => (
                        <Tag key={p} tone="perm">
                          {p}
                        </Tag>
                      ))
                    )}
                  </td>
                  <td className="px-3 py-2 font-mono text-xs text-zinc-500">{u.created_at}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </div>
  );
}
