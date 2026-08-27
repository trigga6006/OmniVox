// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  getMeetingProviderSettings: vi.fn(),
  getMeetingUsageStats: vi.fn(),
  getOpenRouterStatus: vi.fn(),
  listOpenRouterModels: vi.fn(),
  listMeetingAiUsage: vi.fn(),
  saveMeetingProviderSettings: vi.fn(),
  saveOpenRouterKey: vi.fn(),
  removeOpenRouterKey: vi.fn(),
  openOpenRouterCredits: vi.fn(),
  openOpenRouterKeys: vi.fn(),
}));

vi.mock("@/lib/tauri", () => mocks);

import { OpenRouterPanel } from "./OpenRouterPanel";

// jsdom has no scrollIntoView; the kit Select calls it to keep the active
// option in view. Stub it so opening a Select doesn't throw in tests.
Element.prototype.scrollIntoView ??= () => {};

/** The kit Select is a listbox popover, not a native <select>: open, then pick. */
function pickOption(triggerLabel: string, optionName: string | RegExp) {
  fireEvent.click(screen.getByLabelText(triggerLabel));
  fireEvent.click(screen.getByRole("option", { name: optionName }));
}

const settings = {
  provider: "openrouter",
  model: "test/value",
  monthly_budget: 1,
  per_meeting_budget: 0.05,
  max_prompt_price: 1,
  max_completion_price: 3,
  zdr_only: true,
  deny_data_collection: true,
  auto_suggest: true,
  auto_summarize: true,
  summary_preset: "general" as const,
  custom_instructions: "",
  transcription_mode: "after_meeting" as const,
  routing_preference: "price" as const,
};

const valueModel = {
  id: "test/value",
  name: "Value Model",
  description: "",
  context_length: 64_000,
  prompt_price_million: 0.5,
  completion_price_million: 2,
  request_price: 0,
  supports_structured_output: true,
  expiration_date: null,
  intelligence_index: 62.5,
};

const blockedModel = {
  ...valueModel,
  id: "test/blocked",
  name: "Blocked Model",
  prompt_price_million: 2,
};

describe("OpenRouterPanel model planning", () => {
  beforeEach(() => {
    mocks.getMeetingProviderSettings.mockReset().mockResolvedValue(settings);
    mocks.getMeetingUsageStats.mockReset().mockResolvedValue({ month_spend: 0.25, request_count: 4, prompt_tokens: 10_000, completion_tokens: 2_000 });
    mocks.getOpenRouterStatus.mockReset().mockResolvedValue({
      key_present: true,
      connected: true,
      label: "OmniVox",
      limit: 10,
      limit_remaining: 9,
      limit_reset: null,
      usage: 1,
      usage_daily: 0,
      usage_weekly: 0,
      usage_monthly: 0.25,
      is_free_tier: false,
      is_management_key: false,
      expires_at: null,
      error: null,
    });
    mocks.listOpenRouterModels.mockReset().mockResolvedValue([valueModel, blockedModel]);
    mocks.listMeetingAiUsage.mockReset().mockResolvedValue([
      { id: "usage-1", meeting_id: "meeting-1", meeting_title: "Product sync", request_kind: "summary", provider: "OpenRouter", model: "test/value", cost: 0.01, prompt_tokens: 1_000, completion_tokens: 200, created_at: "2026-08-12T12:00:00Z" },
      { id: "usage-2", meeting_id: "meeting-1", meeting_title: "Product sync", request_kind: "question", provider: "OpenRouter", model: "test/value", cost: 0.002, prompt_tokens: 300, completion_tokens: 50, created_at: "2026-08-12T12:10:00Z" },
    ]);
    mocks.saveMeetingProviderSettings.mockReset().mockResolvedValue(undefined);
  });

  afterEach(cleanup);

  it("compares models against duration and configured guardrails", async () => {
    render(<OpenRouterPanel />);

    expect(await screen.findByText("Fits your current cost and context guardrails")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: /Value Model/ }));

    expect((await screen.findByLabelText("Meeting length estimate")).textContent).toContain("60 minutes");
    expect(screen.getByText("Blocked Model")).toBeTruthy();
    expect(screen.getAllByText("Blocked").length).toBeGreaterThan(0);

    fireEvent.click(screen.getByRole("checkbox", { name: "Show only models within my guardrails" }));
    expect(screen.queryByText("Blocked Model")).toBeNull();

    pickOption("Meeting length estimate", "120 minutes");
    expect(screen.getByLabelText("Meeting length estimate").textContent).toContain("120 minutes");
  });

  it("persists a model selected from the comparison list", async () => {
    render(<OpenRouterPanel />);
    await screen.findByText("Fits your current cost and context guardrails");
    fireEvent.click(screen.getByRole("button", { name: /Value Model/ }));
    fireEvent.click(await screen.findByRole("button", { name: /Blocked Model/ }));

    await waitFor(() => expect(mocks.saveMeetingProviderSettings).toHaveBeenCalledWith({
      ...settings,
      model: "test/blocked",
    }));
  });

  it("persists an explicit OpenRouter provider-routing strategy", async () => {
    render(<OpenRouterPanel />);
    await screen.findByText("Fits your current cost and context guardrails");
    fireEvent.click(screen.getByRole("button", { name: /Cost, routing & privacy controls/ }));
    pickOption("OpenRouter provider routing", "Fastest output");

    await waitFor(() => expect(mocks.saveMeetingProviderSettings).toHaveBeenCalledWith({
      ...settings,
      routing_preference: "throughput",
    }));
  });

  it("shows exact request costs grouped by meeting and filters the ledger", async () => {
    render(<OpenRouterPanel />);
    fireEvent.click(await screen.findByRole("button", { name: "View activity" }));

    expect(await screen.findByText("Product sync")).toBeTruthy();
    expect(screen.getByText(/2 requests.*1,550 tokens/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Requests" }));
    expect(screen.getByText("summary")).toBeTruthy();
    expect(screen.getByText("question")).toBeTruthy();

    pickOption("Filter AI activity", "Questions");
    expect(screen.queryByText("summary")).toBeNull();
    expect(screen.getByText("question")).toBeTruthy();
  });
});
