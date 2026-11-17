import { describe, expect, it } from "vitest";
import { sampleEvent, sampleSchedule } from "./sample";
import { formatTime, groupByDay, hackingCountdown, upNext } from "./schedule";

const tz = sampleEvent.timeZone;

describe("upNext", () => {
  it("skips items that have ended and keeps start order", () => {
    const now = new Date("2027-02-20T19:45:00-06:00");
    expect(upNext(sampleSchedule, now, 3).map((item) => item.id)).toEqual([
      "llm-agents",
      "dinner",
      "smash",
    ]);
  });
});

describe("groupByDay", () => {
  it("groups by the event's local day, not UTC", () => {
    // The ramen bar starts at midnight Chicago time, which is 06:00 UTC.
    const days = groupByDay(sampleSchedule, tz);
    expect(days.map((d) => d.day)).toEqual([
      "Saturday, Feb 20",
      "Sunday, Feb 21",
    ]);
    expect(days[1]?.items[0]?.id).toBe("ramen");
  });
});

describe("formatTime", () => {
  it("shows times in the event's zone", () => {
    expect(formatTime("2027-02-21T01:30:00Z", tz)).toBe("7:30 PM");
  });
});

describe("hackingCountdown", () => {
  it("counts down to the start, then the end", () => {
    expect(
      hackingCountdown(sampleEvent, new Date("2027-02-18T11:00:00-06:00")),
    ).toEqual({
      label: "Hacking starts in",
      value: "2d 01:00:00",
    });
    expect(
      hackingCountdown(sampleEvent, new Date("2027-02-20T14:45:51-06:00")),
    ).toEqual({
      label: "Hacking ends in",
      value: "21:14:09",
    });
  });

  it("says when hacking is over", () => {
    expect(
      hackingCountdown(sampleEvent, new Date("2027-02-21T12:00:00-06:00"))
        .label,
    ).toBe("Hacking is over");
  });
});
