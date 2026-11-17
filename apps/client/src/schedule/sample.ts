// SAMPLE DATA. The real dates and sessions aren't set yet; replace this with
// the backend's schedule once it exists. Bundled so the app works offline.
import type { EventInfo, ScheduleItem } from "./types";

export const sampleEvent: EventInfo = {
  name: "WiCS Geaux Hack 2027",
  timeZone: "America/Chicago",
  hackingStartsAt: "2027-02-20T12:00:00-06:00",
  hackingEndsAt: "2027-02-21T12:00:00-06:00",
};

export const sampleSchedule: ScheduleItem[] = [
  {
    id: "checkin",
    title: "Check-in",
    kind: "ceremony",
    startsAt: "2027-02-20T09:00:00-06:00",
    endsAt: "2027-02-20T10:30:00-06:00",
    location: "PFT atrium",
    points: 100,
  },
  {
    id: "opening",
    title: "Opening ceremony",
    kind: "ceremony",
    startsAt: "2027-02-20T10:30:00-06:00",
    endsAt: "2027-02-20T11:30:00-06:00",
    location: "PFT 1100",
  },
  {
    id: "lunch",
    title: "Lunch in the PFT atrium",
    kind: "meal",
    startsAt: "2027-02-20T12:00:00-06:00",
    endsAt: "2027-02-20T13:00:00-06:00",
    location: "PFT atrium",
    points: 25,
  },
  {
    id: "llm-agents",
    title: "Intro to LLM agents with Nexus",
    kind: "workshop",
    startsAt: "2027-02-20T19:30:00-06:00",
    endsAt: "2027-02-20T20:30:00-06:00",
    location: "PFT 1240",
    points: 50,
  },
  {
    id: "dinner",
    title: "Dinner in the PFT atrium",
    kind: "meal",
    startsAt: "2027-02-20T20:00:00-06:00",
    endsAt: "2027-02-20T21:00:00-06:00",
    location: "PFT atrium",
    points: 25,
  },
  {
    id: "smash",
    title: "Smash Bros in the lounge",
    kind: "minigame",
    startsAt: "2027-02-20T22:00:00-06:00",
    endsAt: "2027-02-20T23:30:00-06:00",
    location: "PFT lounge",
    points: 25,
  },
  {
    id: "ramen",
    title: "Midnight ramen bar",
    kind: "social",
    startsAt: "2027-02-21T00:00:00-06:00",
    endsAt: "2027-02-21T01:00:00-06:00",
    location: "PFT atrium",
    points: 25,
  },
  {
    id: "judging",
    title: "Presentations and judging",
    kind: "judging",
    startsAt: "2027-02-21T13:00:00-06:00",
    endsAt: "2027-02-21T15:30:00-06:00",
    location: "Assigned rooms",
  },
  {
    id: "closing",
    title: "Closing ceremony and the ending reveal",
    kind: "ceremony",
    startsAt: "2027-02-21T16:00:00-06:00",
    endsAt: "2027-02-21T17:00:00-06:00",
    location: "PFT 1100",
  },
];
