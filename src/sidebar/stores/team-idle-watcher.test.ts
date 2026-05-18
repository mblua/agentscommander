// @vitest-environment jsdom
//
// Pure-helper tests for the focused-WG suppression + grace period
// behavior introduced in #254. We exercise `shouldSuppressBeep` and
// `updateGraceOnFocusChange` directly — no SolidJS root, no store
// mocks, no fake timers. The wiring inside `startTeamIdleWatcher`'s
// createEffect that calls these helpers is audited via code review
// and the manual smoke test, not unit-mocked here.
//
// jsdom is required because importing this module evaluates the
// file-scope `createSignal` and pulls in the sibling stores, which
// transitively touch `window.location` in `transport-ws.ts`.

import { describe, it, expect } from "vitest";
import {
  GRACE_MS,
  shouldSuppressBeep,
  updateGraceOnFocusChange,
} from "./team-idle-watcher";

describe("shouldSuppressBeep (#254)", () => {
  it("suppresses the focused workgroup", () => {
    expect(shouldSuppressBeep("A", "A", new Map(), 0)).toBe(true);
  });

  it("does NOT suppress non-focused workgroups with no grace entry", () => {
    expect(shouldSuppressBeep("B", "A", new Map(), 0)).toBe(false);
  });

  it("suppresses inside an active grace window", () => {
    const grace = new Map<string, number>([["A", 4000]]);
    expect(shouldSuppressBeep("A", "B", grace, 2000)).toBe(true);
  });

  it("does NOT suppress once the grace window has expired", () => {
    const grace = new Map<string, number>([["A", 4000]]);
    expect(shouldSuppressBeep("A", "B", grace, 5000)).toBe(false);
  });
});

describe("updateGraceOnFocusChange (#254)", () => {
  it("fast switch A→B→A re-arms B's grace and suppresses B inside the window", () => {
    const grace = new Map<string, number>();
    let prev: string | null = null;

    // T=0: initial focus arrives on A. previousFocusedWg was null,
    // so no grace is armed yet.
    prev = updateGraceOnFocusChange(prev, "A", grace, 0, GRACE_MS);
    expect(prev).toBe("A");
    expect(grace.size).toBe(0);

    // T=0: A → B. previousFocusedWg = "A" is left behind: grace
    // armed for A until 0 + 4000 = 4000.
    prev = updateGraceOnFocusChange(prev, "B", grace, 0, GRACE_MS);
    expect(prev).toBe("B");
    expect(grace.get("A")).toBe(4000);

    // T=500: B → A. previousFocusedWg = "B" leaves: grace for B
    // until 500 + 4000 = 4500.
    prev = updateGraceOnFocusChange(prev, "A", grace, 500, GRACE_MS);
    expect(prev).toBe("A");
    expect(grace.get("B")).toBe(4500);

    // T=3000: B is idle, but B is non-focused and still inside its
    // grace window (until 4500) → suppressed.
    expect(shouldSuppressBeep("B", "A", grace, 3000)).toBe(true);

    // T=5000: past B's grace → would beep.
    expect(shouldSuppressBeep("B", "A", grace, 5000)).toBe(false);
  });

  it("alt-tab out (focus → null) arms grace for the previously focused WG", () => {
    const grace = new Map<string, number>();

    // Established focus on A.
    const afterTabOut = updateGraceOnFocusChange(
      "A",
      null,
      grace,
      0,
      GRACE_MS,
    );

    expect(afterTabOut).toBeNull();
    expect(grace.get("A")).toBe(4000);

    // T=2000: A becomes idle while alt-tabbed out → suppressed
    // (grace still active).
    expect(shouldSuppressBeep("A", null, grace, 2000)).toBe(true);
  });

  it("alt-tab back in (focus = A) restores focused-WG suppression for A", () => {
    const grace = new Map<string, number>([["A", 4000]]);

    // The previous tick left focus at null with A's grace armed.
    // Now the window regains focus on A.
    const afterTabIn = updateGraceOnFocusChange(
      null,
      "A",
      grace,
      6000,
      GRACE_MS,
    );

    expect(afterTabIn).toBe("A");
    // previousFocusedWg was null, so no grace was armed for the
    // "left" side — and the existing A grace entry is untouched.
    expect(grace.get("A")).toBe(4000);

    // T=6000: A is focused again → suppressed by the focused-WG rule
    // (independent of A's now-expired grace).
    expect(shouldSuppressBeep("A", "A", grace, 6000)).toBe(true);
  });

  it("no-op when focus does not change", () => {
    const grace = new Map<string, number>();
    const result = updateGraceOnFocusChange("A", "A", grace, 1000, GRACE_MS);
    expect(result).toBe("A");
    expect(grace.size).toBe(0);
  });
});
