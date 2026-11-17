// Everything on the page that isn't settled yet lives here, so it's easy to fill in.

export const APP_URL = "https://app.geauxhack.com";

/**
 * While false, the page asks for an email to notify instead of linking to the
 * application (like HackMIT's page between events). Flip it when PR 1.9 ships.
 */
export const REGISTRATION_OPEN = false;
// TODO: switch to the registration route once PR 1.9 ships.
export const APPLY_URL = APP_URL;

/**
 * Where the "notify me" form posts { email }. Same origin as the app's API.
 * The endpoint doesn't exist yet (see handoffs/josh.md); until it does the
 * form shows its error message.
 */
export const NOTIFY_ENDPOINT = `${APP_URL}/api/v1/notify`;

// TODO: placeholder until the sponsorship inbox exists.
export const SPONSOR_EMAIL = "sponsors@geauxhack.com";

export const EVENT_DATES: string | null = null;
export const VENUE = "Patrick F. Taylor Hall, LSU";
