import { describe, expect, it } from "vitest";
import type { MeetingProviderSettings, MeetingUsageStats, OpenRouterModel } from "@/lib/tauri";
import {
  assessModelForMeeting,
  estimateMeetingTokens,
  estimateModelMeetingCost,
  estimatedMeetingsRemaining,
  sortModelsForCatalog,
} from "./modelCatalog";

const settings: MeetingProviderSettings = {
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
  summary_preset: "general",
  custom_instructions: "",
  transcription_mode: "after_meeting",
  routing_preference: "price",
};

const usage: MeetingUsageStats = {
  month_spend: 0.25,
  request_count: 4,
  prompt_tokens: 10_000,
  completion_tokens: 2_000,
};

function model(overrides: Partial<OpenRouterModel> = {}): OpenRouterModel {
  return {
    id: "test/value",
    name: "Value model",
    description: "",
    context_length: 64_000,
    prompt_price_million: 0.5,
    completion_price_million: 2,
    request_price: 0,
    supports_structured_output: true,
    expiration_date: null,
    intelligence_index: 55,
    ...overrides,
  };
}

describe("meeting model catalog planning", () => {
  it("scales token and cost estimates with meeting duration", () => {
    const short = estimateMeetingTokens(30);
    const long = estimateMeetingTokens(120);
    expect(long.promptTokens).toBeGreaterThan(short.promptTokens);
    expect(long.completionTokens).toBeGreaterThan(short.completionTokens);
    expect(estimateModelMeetingCost(model(), 120)).toBeGreaterThan(estimateModelMeetingCost(model(), 30));
  });

  it("includes per-request, input, and output prices", () => {
    const candidate = model({ request_price: 0.01, prompt_price_million: 1, completion_price_million: 2 });
    const plan = estimateMeetingTokens(60);
    expect(estimateModelMeetingCost(candidate, 60)).toBeCloseTo(
      0.01 + plan.promptTokens / 1_000_000 + (plan.completionTokens * 2) / 1_000_000,
    );
  });

  it("explains every guardrail that would block a model", () => {
    const assessment = assessModelForMeeting(model({
      context_length: 5_000,
      prompt_price_million: 2,
      completion_price_million: 4,
      request_price: 0.06,
    }), { ...settings, monthly_budget: 0.3 }, usage, 60);

    expect(assessment.fits).toBe(false);
    expect(assessment.reasons).toEqual(expect.arrayContaining([
      expect.stringContaining("context"),
      expect.stringContaining("Input price"),
      expect.stringContaining("Output price"),
      expect.stringContaining("meeting limit"),
      expect.stringContaining("monthly budget"),
    ]));
  });

  it("keeps compatible models ahead of blocked models when sorting", () => {
    const cheapButBlocked = model({ id: "blocked", name: "Blocked", prompt_price_million: 1.5, completion_price_million: 0 });
    const compatible = model({ id: "fits", name: "Fits", prompt_price_million: 0.8 });
    expect(sortModelsForCatalog([cheapButBlocked, compatible], "cost", settings, usage, 60).map(({ id }) => id))
      .toEqual(["fits", "blocked"]);
  });

  it("sorts compatible models by benchmark quality and quality per dollar", () => {
    const premium = model({ id: "premium", name: "Premium", intelligence_index: 80, request_price: 0.02 });
    const value = model({ id: "value", name: "Value", intelligence_index: 70, request_price: 0.001 });
    expect(sortModelsForCatalog([value, premium], "quality", settings, usage, 60).map(({ id }) => id))
      .toEqual(["premium", "value"]);
    expect(sortModelsForCatalog([premium, value], "value", settings, usage, 60).map(({ id }) => id))
      .toEqual(["value", "premium"]);
  });

  it("projects remaining summaries from the local monthly guardrail", () => {
    const candidate = model({ request_price: 0.1, prompt_price_million: 0, completion_price_million: 0 });
    expect(estimatedMeetingsRemaining(candidate, settings, usage, 60)).toBe(7);
    expect(estimatedMeetingsRemaining(candidate, { ...settings, monthly_budget: 0 }, usage, 60)).toBeNull();
  });
});
