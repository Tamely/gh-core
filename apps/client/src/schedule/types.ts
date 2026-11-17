// The shape the backend's schedule endpoint should return (see handoffs/josh.md).

export type EventKind =
  "workshop" | "meal" | "social" | "minigame" | "ceremony" | "judging";

export type ScheduleItem = {
  id: string;
  title: string;
  kind: EventKind;
  /** ISO 8601 with offset, e.g. "2027-02-20T19:30:00-06:00". */
  startsAt: string;
  endsAt: string;
  location: string;
  /** Team points for scanning in, if any. */
  points?: number;
};

export type EventInfo = {
  name: string;
  /** IANA zone used to display every time, regardless of the device's zone. */
  timeZone: string;
  hackingStartsAt: string;
  hackingEndsAt: string;
};
