import { describe, expect, test } from "bun:test";
import { mergeEvents } from "./events";
import type { LogEvent } from "./types";

const ev = (n: number, shots: string[] = []): LogEvent => ({
  id: `e${n}`,
  at_ms: n,
  kind: "unlock_ok",
  detail: "",
  shots,
});
const ids = (list: LogEvent[]) => list.map((e) => e.id);

describe("mergeEvents", () => {
  test("a refreshed first page keeps the older pages already loaded", () => {
    const loaded = [ev(5), ev(4), ev(3), ev(2), ev(1)];
    expect(ids(mergeEvents([ev(6), ev(5), ev(4)], loaded))).toEqual(["e6", "e5", "e4", "e3", "e2", "e1"]);
  });

  test("the refreshed copy wins, so evidence that arrived later shows up", () => {
    const merged = mergeEvents([ev(2, ["a-cam.jpg"])], [ev(2), ev(1)]);
    expect(merged[0].shots).toEqual(["a-cam.jpg"]);
    expect(merged).toHaveLength(2);
  });

  test("a next page that overlaps after new events arrived adds no duplicates", () => {
    expect(ids(mergeEvents([ev(6), ev(5), ev(4)], [ev(4), ev(3)]))).toEqual(["e6", "e5", "e4", "e3"]);
  });

  test("empty inputs", () => {
    expect(mergeEvents([], [])).toEqual([]);
    expect(ids(mergeEvents([], [ev(1)]))).toEqual(["e1"]);
  });
});
