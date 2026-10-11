import { describe, expect, test } from "bun:test";
import { formatCountdown, formatWhen, KINDS, orderShots, shotKind, spokenCountdown } from "./format";

describe("formatCountdown", () => {
  test("minutes and zero-padded seconds", () => {
    expect(formatCountdown(754)).toBe("12:34");
    expect(formatCountdown(59)).toBe("0:59");
    expect(formatCountdown(0)).toBe("0:00");
  });
  test("never negative, and switches to hours past 60 minutes", () => {
    expect(formatCountdown(-5)).toBe("0:00");
    expect(formatCountdown(3 * 3600 + 5 * 60)).toBe("3h 05min");
  });
  test("spoken form rounds up so it never claims time that is gone", () => {
    expect(spokenCountdown(61)).toBe("2 minutos");
    expect(spokenCountdown(30)).toBe("menos de 1 minuto");
  });
});

describe("formatWhen", () => {
  test("same day says 'hoje', other days carry the date", () => {
    const now = new Date(2026, 9, 11, 12, 0, 0);
    expect(formatWhen(new Date(2026, 9, 11, 8, 30, 5).getTime(), now)).toMatch(/^hoje 08:30:05$/);
    expect(formatWhen(new Date(2026, 9, 10, 23, 1, 2).getTime(), now)).toMatch(/^10\/10 23:01:02$/);
  });
});

describe("shots", () => {
  test("the webcam frame is listed before screen captures", () => {
    const names = ["a-screen1.jpg", "a-cam.jpg", "a-screen0.jpg"];
    expect(orderShots(names)).toEqual(["a-cam.jpg", "a-screen0.jpg", "a-screen1.jpg"]);
    expect(shotKind("a-cam.jpg")).toBe("cam");
    expect(shotKind("a-screen0.jpg")).toBe("screen");
  });
  test("every kind the backend can emit has a label", () => {
    expect(Object.keys(KINDS)).toHaveLength(13);
  });
});
