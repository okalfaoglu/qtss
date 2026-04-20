import { createBrowserRouter, Navigate, Outlet } from "react-router-dom";

import { Layout } from "./components/Layout";
import { isAuthenticated } from "./lib/auth";
import { Login } from "./pages/Login";
import { Dashboard } from "./pages/Dashboard";
import { Risk } from "./pages/Risk";
import { Blotter } from "./pages/Blotter";
import { Backtest } from "./pages/Backtest";
import { Setups } from "./pages/Setups";
import { SetupRejections } from "./pages/SetupRejections";
import { TrainingSet } from "./pages/TrainingSet";
import { Models } from "./pages/Models";
import { Drift } from "./pages/Drift";
import { MonteCarlo } from "./pages/MonteCarlo";
import { Regime } from "./pages/Regime";
import { Chart } from "./pages/Chart";
import { Detections } from "./pages/Detections";
import { Tbm } from "./pages/Tbm";
import { Onchain } from "./pages/Onchain";
import { Scenarios } from "./pages/Scenarios";
import { Config } from "./pages/Config";
import { Reconcile } from "./pages/Reconcile";
import { AiDecisions } from "./pages/AiDecisions";
import { Audit } from "./pages/Audit";
import { Users } from "./pages/Users";
import { EngineSymbols } from "./pages/EngineSymbols";
import { WaveTree } from "./pages/WaveTree";
import { Wyckoff } from "./pages/Wyckoff";
import { AiShadow } from "./pages/AiShadow";
import { FeatureInspector } from "./pages/FeatureInspector";
import { Reports } from "./pages/Reports";
import { ReversalRadar } from "./pages/ReversalRadar";
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
      { path: "backtest", element: <Backtest /> },
      { path: "setups", element: <Setups /> },
      { path: "setup-rejections", element: <SetupRejections /> },
      { path: "training-set", element: <TrainingSet /> },
      { path: "models", element: <Models /> },
      { path: "drift", element: <Drift /> },
      { path: "montecarlo", element: <MonteCarlo /> },
      { path: "regime", element: <Regime /> },
      { path: "chart", element: <Chart /> },
      { path: "detections", element: <Detections /> },
      { path: "tbm", element: <Tbm /> },
      { path: "onchain", element: <Onchain /> },
      { path: "scenarios", element: <Scenarios /> },
      { path: "config", element: <Config /> },
      { path: "reconcile", element: <Reconcile /> },
      { path: "ai-decisions", element: <AiDecisions /> },
      { path: "audit", element: <Audit /> },
      { path: "users", element: <Users /> },
      { path: "engine-symbols", element: <EngineSymbols /> },
      { path: "wave-tree", element: <WaveTree /> },
      { path: "wyckoff", element: <Wyckoff /> },
      { path: "ai-shadow", element: <AiShadow /> },
      { path: "feature-inspector", element: <FeatureInspector /> },
      { path: "reports/backtest", element: <Reports /> },
      { path: "reversal-radar", element: <ReversalRadar /> },
      { path: "symbols", element: <Symbols /> },
    ],
  },
  { path: "/", element: <Navigate to="/v2/dashboard" replace /> },
  { path: "*", element: <Navigate to="/v2/dashboard" replace /> },
]);
