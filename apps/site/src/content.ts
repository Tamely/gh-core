// Everything on the page that isn't settled yet lives here, so it's easy to fill in.

export const APP_URL = "https://app.geauxhack.org";
// TODO: switch to the registration route once PR 1.9 ships.
export const APPLY_URL = APP_URL;
// TODO: placeholder until the sponsorship inbox exists.
export const SPONSOR_EMAIL = "sponsors@geauxhack.org";

/** Null dates render as "To be announced". */
export const milestones: { label: string; date: string | null }[] = [
  { label: "Applications open", date: null },
  { label: "Application deadline", date: null },
  { label: "Decisions go out", date: null },
  { label: "Confirm your spot", date: null },
  { label: "Geaux Hack 2027", date: null },
];

export const EVENT_DATES: string | null = null;
export const VENUE = "Patrick F. Taylor Hall, LSU";
