import { describe, expect, it } from "vitest";

import { discoverLanes, firstLaneWithContent } from "./LibraryPersonalization";
import type { LocalPersonalization, PersonalizedRecommendationItem } from "./types";

describe("firstLaneWithContent", () => {
  it("opens Suggested when suggestions exist", () => {
    expect(firstLaneWithContent(personalization({ becauseYou: [item("RJ1")] }))).toBe("suggested");
  });

  it("skips empty Suggested and opens the lane that has content", () => {
    expect(firstLaneWithContent(personalization({ favorites: [work("RJ2")] }))).toBe("favorites");
    expect(firstLaneWithContent(personalization({ voiceMix: [item("RJ3")] }))).toBe("voiceMix");
  });

  it("prefers Suggested when several lanes have content", () => {
    const filled = personalization({
      becauseYou: [item("RJ1")],
      favorites: [work("RJ2")],
      voiceMix: [item("RJ3")],
    });
    expect(firstLaneWithContent(filled)).toBe("suggested");
  });

  it("falls back to Suggested when every lane is empty", () => {
    expect(firstLaneWithContent(personalization({}))).toBe("suggested");
  });

  it("only ever returns a lane the tablist renders", () => {
    const cases = [
      personalization({}),
      personalization({ favorites: [work("RJ2")] }),
      personalization({ voiceMix: [item("RJ3")] }),
      personalization({ becauseYou: [item("RJ1")] }),
    ];
    for (const value of cases) {
      expect(discoverLanes).toContain(firstLaneWithContent(value));
    }
  });
});

function personalization(overrides: Partial<LocalPersonalization>): LocalPersonalization {
  return {
    favorites: [],
    becauseYou: [],
    voiceMix: [],
    activityWorkCount: 0,
    voiceActivityWorkCount: 0,
    becauseYouMinimum: 2,
    voiceMixMinimum: 2,
    ...overrides,
  } as LocalPersonalization;
}

function work(code: string): PersonalizedRecommendationItem["work"] {
  return { code, title: code } as PersonalizedRecommendationItem["work"];
}

function item(code: string): PersonalizedRecommendationItem {
  return { work: work(code), anchors: [] } as unknown as PersonalizedRecommendationItem;
}
