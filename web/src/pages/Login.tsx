import { FormEvent, useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";

import { getSuggestedEmail, login } from "../lib/auth";

export function Login() {
  const navigate = useNavigate();
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  // Pre-fill email from the bootstrap hint so first-time admins do not have
  // to dig through the API config to find the seeded account.
  useEffect(() => {
    let cancelled = false;
    void getSuggestedEmail().then((suggested) => {
      if (!cancelled && suggested) {
        setEmail((prev) => prev || suggested);
      }
    });
    return () => {
      cancelled = true;
    };
  }, []);

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();
    setError(null);
    setSubmitting(true);
    try {
      await login(email, password);
      navigate("/v2/dashboard", { replace: true });
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div className="flex min-h-screen items-center justify-center px-4">
      <form
        onSubmit={handleSubmit}
        className="w-full max-w-sm space-y-4 rounded-lg border border-zinc-800 bg-zinc-900/60 p-6 shadow-xl"
      >
        <div>
          <h1 className="text-xl font-semibold text-zinc-100">QTSS v2</h1>
          <p className="text-sm text-zinc-400">Sign in to continue</p>
        </div>
        <label className="block text-sm">
          <span className="mb-1 block text-zinc-300">Email</span>
          <input
            type="email"
            required
            value={email}
            onChange={(e) => setEmail(e.target.value)}
            className="w-full rounded border border-zinc-700 bg-zinc-950 px-3 py-2 text-zinc-100 outline-none focus:border-emerald-500"
          />
        </label>
        <label className="block text-sm">
          <span className="mb-1 block text-zinc-300">Password</span>
          <input
            type="password"
            required
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            className="w-full rounded border border-zinc-700 bg-zinc-950 px-3 py-2 text-zinc-100 outline-none focus:border-emerald-500"
          />
        </label>
        {error && (
          <div className="rounded border border-red-800 bg-red-950/40 px-3 py-2 text-xs text-red-300">
            {error}
          </div>
        )}
        <button
          type="submit"
          disabled={submitting}
          className="w-full rounded bg-emerald-500 px-3 py-2 text-sm font-medium text-zinc-950 hover:bg-emerald-400 disabled:opacity-50"
        >
          {submitting ? "Signing in…" : "Sign in"}
        </button>
      </form>
    </div>
  );
}
