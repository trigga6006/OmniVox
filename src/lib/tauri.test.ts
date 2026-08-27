import { describe, expect, it } from "vitest";
import {
  createCompletionGenerationGate,
  createGenerationGate,
  createPerGenerationLifecycleGate,
} from "./tauri";

describe("capture generation gate", () => {
  it("accepts all events from the current generation", () => {
    const gate = createGenerationGate();

    expect(gate.accept(7)).toBe(true);
    expect(gate.accept(7)).toBe(true);
    expect(gate.latest()).toBe(7);
  });

  it("rejects late events after a newer capture is observed", () => {
    const gate = createGenerationGate();

    expect(gate.accept(7)).toBe(true);
    expect(gate.accept(8)).toBe(true);
    expect(gate.accept(7)).toBe(false);
    expect(gate.latest()).toBe(8);
  });

  it("rejects malformed generations without advancing", () => {
    const gate = createGenerationGate(4);

    expect(gate.accept(-1)).toBe(false);
    expect(gate.accept(Number.NaN)).toBe(false);
    expect(gate.accept(Number.MAX_SAFE_INTEGER + 1)).toBe(false);
    expect(gate.latest()).toBe(4);
  });
});

describe("completion generation gate", () => {
  it("delivers an older completion after a newer one", () => {
    const gate = createCompletionGenerationGate();

    expect(gate.accept(8)).toBe(true);
    expect(gate.accept(7)).toBe(true);
    expect(gate.hasDelivered(7)).toBe(true);
    expect(gate.hasDelivered(8)).toBe(true);
  });

  it("rejects duplicate and malformed completion deliveries", () => {
    const gate = createCompletionGenerationGate();

    expect(gate.accept(7)).toBe(true);
    expect(gate.accept(7)).toBe(false);
    expect(gate.accept(-1)).toBe(false);
    expect(gate.accept(Number.NaN)).toBe(false);
  });
});

describe("per-generation recording lifecycle gate", () => {
  it("delivers an older capture's terminal state after a newer capture starts", () => {
    const gate = createPerGenerationLifecycleGate();

    expect(gate.accept(7, "recording")).toBe(true);
    expect(gate.accept(8, "recording")).toBe(true);
    expect(gate.accept(8, "processing")).toBe(true);
    expect(gate.accept(7, "idle")).toBe(true);
  });

  it("rejects duplicate, regressive, and malformed lifecycle events", () => {
    const gate = createPerGenerationLifecycleGate();

    expect(gate.accept(7, "recording")).toBe(true);
    expect(gate.accept(7, "recording")).toBe(false);
    expect(gate.accept(7, "processing")).toBe(true);
    expect(gate.accept(7, "recording")).toBe(false);
    expect(gate.accept(7, "unknown")).toBe(false);
    expect(gate.accept(-1, "idle")).toBe(false);
  });
});
