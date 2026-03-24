/**
 * Lightweight entry point for the overlay (floating pill) window.
 *
 * This is intentionally minimal — it imports ONLY the overlay components
 * and their direct dependencies (recordingStore, a few Tauri commands).
 * The main app's page components, Sidebar, history, dictionary, etc. are
 * never parsed or loaded, saving ~200-250 MB of WebView2 memory.
 */
import React from "react";
import ReactDOM from "react-dom/client";
import { FloatingPill } from "@/features/overlay/FloatingPill";
import "./styles/globals.css";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <FloatingPill />
  </React.StrictMode>
);
