import { createBrowserRouter, Navigate, Outlet } from "react-router-dom";

import { Layout } from "./components/Layout";
import { isAuthenticated } from "./lib/auth";
import { Login } from "./pages/Login";
import { Dashboard } from "./pages/Dashboard";
import { Risk } from "./pages/Risk";
import { Blotter } from "./pages/Blotter";
import { Strategies } from "./pages/Strategies";
import { MonteCarlo } from "./pages/MonteCarlo";
import { Regime } from "./pages/Regime";
import { Chart } from "./pages/Chart";
import { Scenarios } from "./pages/Scenarios";

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

export const router = createBrowserRouter([
  { path: "/login", element: <Login /> },
  {
    path: "/v2",
    element: <ProtectedRoute />,
    children: [
      { index: true, element: <Navigate to="dashboard" replace /> },
      { path: "dashboard", element: <Dashboard /> },
      { path: "risk", element: <Risk /> },
      { path: "blotter", element: <Blotter /> },
      { path: "strategies", element: <Strategies /> },
      { path: "montecarlo", element: <MonteCarlo /> },
      { path: "regime", element: <Regime /> },
      { path: "chart", element: <Chart /> },
      { path: "scenarios", element: <Scenarios /> },
    ],
  },
  { path: "/", element: <Navigate to="/v2/dashboard" replace /> },
  { path: "*", element: <Navigate to="/v2/dashboard" replace /> },
]);
