import { createBrowserRouter, Navigate, Outlet } from "react-router-dom";

import { Layout } from "./components/Layout";
import { isAuthenticated } from "./lib/auth";
import { Login } from "./pages/Login";
import { Dashboard } from "./pages/Dashboard";
import { Regime } from "./pages/Regime";
import { LuxAlgoChart } from "./pages/LuxAlgoChart";
import { Detections } from "./pages/Detections";
import { Config } from "./pages/Config";
import { Users } from "./pages/Users";
import { EngineSymbols } from "./pages/EngineSymbols";
import { Symbols } from "./pages/Symbols";

// Guard: bounce to /login when there is no token in storage. We deliberately
// keep this check synchronous (localStorage) so the first paint never flashes
// a protected page to an unauthenticated user.
function ProtectedRoute() {
  if (!isAuthenticated()) {
    return <Navigate to="/login" replace />;
  }
  return (
    <Layout>
      <Outlet />
    </Layout>
  );
}

export const router = createBrowserRouter(
  [
    { path: "/login", element: <Login /> },
    {
      path: "/v2",
      element: <ProtectedRoute />,
      children: [
        { index: true, element: <Navigate to="dashboard" replace /> },
        { path: "dashboard", element: <Dashboard /> },
        { path: "regime", element: <Regime /> },
        { path: "chart", element: <LuxAlgoChart /> },
        { path: "detections", element: <Detections /> },
        { path: "config", element: <Config /> },
        { path: "users", element: <Users /> },
        { path: "engine-symbols", element: <EngineSymbols /> },
        { path: "symbols", element: <Symbols /> },
      ],
    },
    { path: "/", element: <Navigate to="/v2/dashboard" replace /> },
    { path: "*", element: <Navigate to="/v2/dashboard" replace /> },
  ],
  {
    future: {
      v7_relativeSplatPath: true,
      v7_fetcherPersist: true,
      v7_normalizeFormMethod: true,
      v7_partialHydration: true,
      v7_skipActionErrorRevalidation: true,
    },
  }
);
