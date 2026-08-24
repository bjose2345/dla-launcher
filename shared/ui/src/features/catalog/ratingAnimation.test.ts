import { describe, expect, it } from "vitest";

import { ratingCountDelay, ratingCountDuration, ratingCountValue } from "./ratingAnimation";

describe("rating count-up", () => {
  it("holds at zero during its entrance delay", () => {
    expect(ratingCountValue(94, 0)).toBe(0);
    expect(ratingCountValue(94, ratingCountDelay)).toBe(0);
  });

  it("uses the website ease-out curve and reaches the target", () => {
    expect(ratingCountValue(100, ratingCountDelay + ratingCountDuration / 2)).toBe(87.5);
    expect(ratingCountValue(94, ratingCountDelay + ratingCountDuration)).toBe(94);
    expect(ratingCountValue(94, ratingCountDelay + ratingCountDuration + 100)).toBe(94);
  });
});
