import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { ErrorBoundary } from "@/components/ErrorBoundary";
import { MeetingWidget } from "@/features/meetings/MeetingWidget";
import "@/styles/globals.css";

createRoot(document.getElementById("root")!).render(<StrictMode><ErrorBoundary><MeetingWidget /></ErrorBoundary></StrictMode>);
