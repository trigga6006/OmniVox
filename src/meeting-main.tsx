import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { Providers } from "@/app/providers";
import { ErrorBoundary } from "@/components/ErrorBoundary";
import { MeetingDrawer } from "@/features/meetings/MeetingDrawer";
import "@/styles/globals.css";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <ErrorBoundary>
      <Providers><MeetingDrawer /></Providers>
    </ErrorBoundary>
  </StrictMode>
);
