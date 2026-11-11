import { useEffect } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { Sidebar } from "./components/Sidebar";
import { JobsTray } from "./components/JobsTray";
import { Toasts } from "./components/Toasts";
import { NewProjectDialog } from "./components/NewProjectDialog";
import { ProfileDialog } from "./components/ProfileDialog";
import { TopMenuBar } from "./shell/TopMenuBar";
import { ExportDialog } from "./export/ExportDialog";
import { useApp } from "./stores/app";
import { api, onJob } from "./services/api";
import Home from "./pages/Home";
import Workstation from "./pages/Editor";
import AudioIntel from "./pages/AudioIntel";
import MediaLibrary from "./pages/MediaLibrary";
import Presets from "./pages/Presets";
import ExportCenter from "./pages/ExportCenter";
import Projects from "./pages/Projects";
import Settings from "./pages/Settings";
import NicheExplorer from "./pages/NicheExplorer";
import Planned from "./pages/Planned";
import Profiles from "./pages/Profiles";
import ClipDiscovery from "./pages/ClipDiscovery";

export default function App() {
  const route = useApp((s) => s.route);
  const upsertJob = useApp((s) => s.upsertJob);
  const refreshProject = useApp((s) => s.refreshProject);
  const qc = useQueryClient();

  useEffect(() => {
    const un = onJob((j) => {
      upsertJob(j);
      if (j.status === "completed") {
        // Jobs mutate project state in Rust; resync the view.
        if (["import", "generate", "render"].includes(j.kind)) void refreshProject();
        if (j.kind === "audio_analysis") void qc.invalidateQueries({ queryKey: ["audio-analysis"] });
        if (j.kind === "video_analysis") void qc.invalidateQueries({ queryKey: ["video-analysis"] });
        if (j.kind === "render") void qc.invalidateQueries({ queryKey: ["recent-renders"] });
        if (j.kind === "import") void qc.invalidateQueries({ queryKey: ["profile-assets"] });
      }
    });
    void api.listJobs().then((js) => js.forEach(upsertJob));
    void refreshProject();
    return () => {
      void un.then((f) => f());
    };
  }, [upsertJob, refreshProject, qc]);

  const page = (() => {
    switch (route) {
      case "home":
        return <Home />;
      case "auto":
      case "editor":
        return <Workstation />;
      case "audio":
        return <AudioIntel />;
      case "library":
        return <MediaLibrary />;
      case "discover":
        return <ClipDiscovery />;
      case "niches":
        return <NicheExplorer />;
      case "presets":
        return <Presets />;
      case "export":
        return <ExportCenter />;
      case "projects":
        return <Projects />;
      case "settings":
        return <Settings />;
      case "profiles":
        return <Profiles />;
      case "trends":
      case "batch":
      case "campaigns":
        return <Planned which={route} />;
    }
  })();

  const workstation = route === "editor" || route === "auto";

  return (
    <div className="flex h-full flex-col">
      <TopMenuBar />
      <div className="flex min-h-0 flex-1">
        {!workstation && <Sidebar />}
        <main className="min-h-0 min-w-0 flex-1 overflow-hidden">{page}</main>
      </div>
      <JobsTray />
      <Toasts />
      <NewProjectDialog />
      <ProfileDialog />
      <ExportDialog />
    </div>
  );
}
