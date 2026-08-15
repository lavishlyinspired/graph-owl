import { useCallback, useEffect, useState } from "react";
import TopNav from "./components/TopNav.jsx";
import PeriodPicker from "./components/PeriodPicker.jsx";
import { LoadingScreen } from "./components/ui.jsx";
import { api } from "./api.js";
import UploadPage from "./pages/UploadPage.jsx";
import MapPage from "./pages/MapPage.jsx";
import ReconcilePage from "./pages/ReconcilePage.jsx";
import IntelligencePage from "./pages/IntelligencePage.jsx";
import ActPage from "./pages/ActPage.jsx";

export default function App() {
  const [page, setPage] = useState("upload");
  const [overview, setOverview] = useState(null);
  const [loading, setLoading] = useState(true);

  const refresh = useCallback(async () => {
    const data = await api.overview();
    setOverview(data);
  }, []);

  useEffect(() => {
    api
      .health()
      .then(() => refresh())
      .catch(() => setOverview(null))
      .finally(() => setLoading(false));
  }, [refresh]);

  const hasData = Boolean(overview?.datasets?.books || overview?.datasets?.gstr2b);
  const period = overview?.period ?? { month: "March", year: 2026 };

  const handleRestart = async () => {
    await api.reset();
    setOverview(await api.overview());
    setPage("upload");
  };

  const handlePeriodChange = (month, year) => {
    setOverview((prev) => (prev ? { ...prev, period: { month, year } } : prev));
  };

  const handleDataLoaded = (data) => {
    setOverview(data);
    setPage("map");
  };

  const handleMapped = async () => {
    const data = await api.reconcile();
    setOverview(data);
    setPage("reconcile");
  };

  if (loading) return <LoadingScreen />;

  return (
    <div className="min-h-screen bg-matcha-bg">
      <TopNav
        page={page}
        hasData={hasData}
        onNavigate={setPage}
        onRestart={handleRestart}
      />

      <div className="max-w-7xl mx-auto px-4 sm:px-6 py-6">
        <div className="flex justify-end mb-4">
          <PeriodPicker
            month={period.month}
            year={period.year}
            onChange={handlePeriodChange}
          />
        </div>

        {page === "upload" && (
          <UploadPage onDataLoaded={handleDataLoaded} />
        )}
        {page === "map" && (
          <MapPage overview={overview} onMapped={handleMapped} onBack={() => setPage("upload")} />
        )}
        {page === "reconcile" && (
          <ReconcilePage overview={overview} onMapping={() => setPage("map")} onIntelligence={() => setPage("intelligence")} />
        )}
        {page === "intelligence" && (
          <IntelligencePage overview={overview} onBack={() => setPage("reconcile")} onAct={() => setPage("act")} />
        )}
        {page === "act" && (
          <ActPage overview={overview} onBack={() => setPage("intelligence")} onRestart={handleRestart} />
        )}
      </div>
    </div>
  );
}
