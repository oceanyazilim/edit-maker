import { describe, expect, it } from "vitest";
import { frameToTicks, snapToFrame, ticksToFrame, timecode, TICKS_PER_SECOND } from "./time";

const fps30 = { num: 30, den: 1 };
const ntsc = { num: 30000, den: 1001 };

describe("timebase", () => {
  it("frame <-> tick is exact for standard rates", () => {
    for (const fps of [fps30, ntsc, { num: 24, den: 1 }, { num: 60, den: 1 }]) {
      for (const n of [0, 1, 29, 1800, 107892]) {
        expect(ticksToFrame(frameToTicks(n, fps), fps)).toBe(n);
      }
    }
  });
  it("snaps to the nearest frame", () => {
    const half = frameToTicks(10, fps30) + TICKS_PER_SECOND / 30 / 2 - 1;
    expect(snapToFrame(half, fps30)).toBe(frameToTicks(10, fps30));
  });
  it("formats timecode", () => {
    expect(timecode(frameToTicks(75, fps30), fps30)).toBe("00:02.15");
    expect(timecode(frameToTicks(30 * 61, fps30), fps30)).toBe("01:01.00");
  });
});
