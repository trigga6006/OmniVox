import type {
  MeetingProviderSettings,
  MeetingUsageStats,
  OpenRouterModel,
} from "@/lib/tauri";

export const MEETING_LENGTH_OPTIONS = [30, 60, 90, 120] as const;

export type ModelCatalogSort = "cost" | "quality" | "value" | "context" | "name";

export interface MeetingTokenPlan {
  promptTokens: number;
  completionTokens: number;
  requiredContextTokens: number;
}

export interface ModelGuardrailAssessment {
  estimatedCost: number;
  plan: MeetingTokenPlan;
  fits: boolean;
  reasons: string[];
}

/**
 * A planning estimate for comparing models, not the paid-request preflight.
 * The backend still prices the actual transcript and blocks every request that
 * exceeds a configured guardrail.
 */
export function estimateMeetingTokens(minutes: number): MeetingTokenPlan {
  const duration = Number.isFinite(minutes) ? Math.min(480, Math.max(5, minutes)) : 60;
  const promptTokens = Math.ceil(2_000 + duration * 215);
  const completionTokens = Math.ceil(Math.min(3_000, 700 + duration * 15));
  return {
    promptTokens,
    completionTokens,
    requiredContextTokens: promptTokens + completionTokens + 1_024,
  };
}

export function estimateModelMeetingCost(model: OpenRouterModel, minutes: number): number {
  const plan = estimateMeetingTokens(minutes);
  return model.request_price
    + (plan.promptTokens * model.prompt_price_million) / 1_000_000
    + (plan.completionTokens * model.completion_price_million) / 1_000_000;
}

export function assessModelForMeeting(
  model: OpenRouterModel,
  settings: MeetingProviderSettings,
  usage: MeetingUsageStats | null,
  minutes: number,
): ModelGuardrailAssessment {
  const plan = estimateMeetingTokens(minutes);
  const estimatedCost = estimateModelMeetingCost(model, minutes);
  const reasons: string[] = [];

  if (model.context_length < plan.requiredContextTokens) {
    reasons.push(`Needs about ${compactTokens(plan.requiredContextTokens)} context`);
  }
  if (model.prompt_price_million > settings.max_prompt_price) {
    reasons.push(`Input price exceeds the $${settings.max_prompt_price.toFixed(2)}/M limit`);
  }
  if (model.completion_price_million > settings.max_completion_price) {
    reasons.push(`Output price exceeds the $${settings.max_completion_price.toFixed(2)}/M limit`);
  }
  if (settings.per_meeting_budget > 0 && estimatedCost > settings.per_meeting_budget) {
    reasons.push(`Estimate exceeds the $${settings.per_meeting_budget.toFixed(2)} meeting limit`);
  }
  if (
    usage
    && settings.monthly_budget > 0
    && usage.month_spend + estimatedCost > settings.monthly_budget
  ) {
    reasons.push("Estimate exceeds the remaining monthly budget");
  }

  return { estimatedCost, plan, fits: reasons.length === 0, reasons };
}

export function sortModelsForCatalog(
  models: OpenRouterModel[],
  sort: ModelCatalogSort,
  settings: MeetingProviderSettings,
  usage: MeetingUsageStats | null,
  minutes: number,
): OpenRouterModel[] {
  return [...models].sort((left, right) => {
    const leftAssessment = assessModelForMeeting(left, settings, usage, minutes);
    const rightAssessment = assessModelForMeeting(right, settings, usage, minutes);
    const fitOrder = Number(rightAssessment.fits) - Number(leftAssessment.fits);
    if (fitOrder !== 0) return fitOrder;

    if (sort === "context") {
      return right.context_length - left.context_length
        || leftAssessment.estimatedCost - rightAssessment.estimatedCost
        || left.name.localeCompare(right.name);
    }
    if (sort === "quality") {
      return benchmarkScore(right) - benchmarkScore(left)
        || leftAssessment.estimatedCost - rightAssessment.estimatedCost
        || left.name.localeCompare(right.name);
    }
    if (sort === "value") {
      return modelValueScore(right, minutes) - modelValueScore(left, minutes)
        || benchmarkScore(right) - benchmarkScore(left)
        || leftAssessment.estimatedCost - rightAssessment.estimatedCost
        || left.name.localeCompare(right.name);
    }
    if (sort === "name") return left.name.localeCompare(right.name);
    return leftAssessment.estimatedCost - rightAssessment.estimatedCost
      || right.context_length - left.context_length
      || left.name.localeCompare(right.name);
  });
}

function benchmarkScore(model: OpenRouterModel): number {
  return model.intelligence_index ?? Number.NEGATIVE_INFINITY;
}

/** Broad benchmark points per estimated meeting dollar. It is a comparison
 * aid, not a claim about meeting-summary accuracy. */
export function modelValueScore(model: OpenRouterModel, minutes: number): number {
  if (model.intelligence_index == null) return Number.NEGATIVE_INFINITY;
  return model.intelligence_index / Math.max(0.001, estimateModelMeetingCost(model, minutes));
}

export function estimatedMeetingsRemaining(
  model: OpenRouterModel,
  settings: MeetingProviderSettings,
  usage: MeetingUsageStats | null,
  minutes: number,
): number | null {
  if (settings.monthly_budget <= 0) return null;
  const cost = estimateModelMeetingCost(model, minutes);
  if (cost <= 0) return null;
  const remaining = Math.max(0, settings.monthly_budget - (usage?.month_spend ?? 0));
  return Math.floor(remaining / cost);
}

export function compactTokens(tokens: number): string {
  if (tokens >= 1_000_000) return `${(tokens / 1_000_000).toFixed(tokens >= 10_000_000 ? 0 : 1)}M`;
  if (tokens >= 1_000) return `${(tokens / 1_000).toFixed(tokens >= 10_000 ? 0 : 1)}K`;
  return String(tokens);
}
