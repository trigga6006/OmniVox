/**
 * Entry point for the Scratchpad window — a small, always-on-top quick-capture
 * surface you dictate into. Kept minimal like the overlay: it imports only the
 * scratchpad UI, not the main app's pages.
 */
import React from "react";
import ReactDOM from "react-dom/client";
import { Scratchpad } from "@/features/scratchpad/Scratchpad";
import "./styles/globals.css";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <Scratchpad />
  </React.StrictMode>
);
