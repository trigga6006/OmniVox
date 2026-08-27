import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { ErrorBoundary } from "@/components/ErrorBoundary";
import { MeetingSuggestion } from "@/features/meetings/MeetingSuggestion";
import "@/styles/globals.css";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <ErrorBoundary>
      <MeetingSuggestion />
    </ErrorBoundary>
  </StrictMode>
);
