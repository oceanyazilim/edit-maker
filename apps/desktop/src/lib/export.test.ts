import { describe, expect, it } from "vitest";
import { fileNameError, joinPath, parseAspect, presetOf, sizeFor, validDimension, withExt } from "./export";

describe("export resolution", () => {
  it("is aspect-aware and even", () => {
    expect(sizeFor(1080, 9, 16)).toEqual({ width: 1080, height: 1920 });
    expect(sizeFor(1080, 16, 9)).toEqual({ width: 1920, height: 1080 });
    expect(sizeFor(720, 1, 1)).toEqual({ width: 720, height: 720 });
    expect(sizeFor(1080, 4, 5)).toEqual({ width: 1080, height: 1350 });
    expect(sizeFor(2160, 9, 16)).toEqual({ width: 2160, height: 3840 });
    const odd = sizeFor(721, 9, 16);
    expect(odd.width % 2).toBe(0);
    expect(odd.height % 2).toBe(0);
  });
  it("maps canvases back to presets", () => {
    expect(presetOf(1080, 1920)).toBe("1080");
    expect(presetOf(3840, 2160)).toBe("2160");
    expect(presetOf(1000, 1000)).toBe("custom");
  });
  it("parses aspect ratios", () => {
    expect(parseAspect("9:16")).toEqual([9, 16]);
    expect(parseAspect("2.39:1")).toEqual([2.39, 1]);
    expect(parseAspect("wide")).toBeNull();
  });
  it("validates dimensions", () => {
    expect(validDimension(1080)).toBe(true);
    expect(validDimension(1081)).toBe(false);
    expect(validDimension(8)).toBe(false);
  });
});

describe("windows file names", () => {
  it("rejects illegal names", () => {
    expect(fileNameError("")).not.toBeNull();
    expect(fileNameError("a:b.mp4")).not.toBeNull();
    expect(fileNameError("what?.mp4")).not.toBeNull();
    expect(fileNameError("clip.")).not.toBeNull();
    expect(fileNameError("CON.mp4")).not.toBeNull();
    expect(fileNameError("lpt1")).not.toBeNull();
  });
  it("accepts normal names", () => {
    expect(fileNameError("Funk Misterio – drop edit v2.mp4")).toBeNull();
    expect(fileNameError("console.mp4")).toBeNull();
  });
  it("fixes extensions and joins paths", () => {
    expect(withExt("edit", "mp4")).toBe("edit.mp4");
    expect(withExt("edit.MP4", "mp4")).toBe("edit.MP4");
    expect(withExt("edit.mp4", "wav")).toBe("edit.wav");
    expect(joinPath("C:\\Users\\x\\exports", "a.mp4")).toBe("C:\\Users\\x\\exports\\a.mp4");
    expect(joinPath("C:\\out\\", "a.mp4")).toBe("C:\\out\\a.mp4");
  });
});
