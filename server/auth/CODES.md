# PR 2 acceptance check

Checked on 2026-11-21, on `ricky/auth-email-codes`. PR 2 covers email/SMS sign-in;
OAuth, device/offline UI and judge-room timing belong to the other PRs.

| Requirement | Evidence / result |
|---|---|
| Six digits from a cryptographically secure source | Email/dev codes use OS `getrandom`, rejection sampling (no modulo bias), and zero-padding to six ASCII digits. Twilio Verify generates production SMS codes; its service must be configured for six digits. |
| Store only a hash | Redis holds a SHA-256 hash of address + code, expiry and attempt count. `redis_keeps_a_hash_not_the_code` checks stored data and the Redis TTL. Production logging of codes is compiled out. |
| Expire after ten minutes | `codes_expire_after_10_minutes` checks the exact boundary with a fake clock; the Redis hash also expires. Production SMS expiry is enforced by Twilio. |
| At most five attempts | `five_tries_per_code` and `parallel_guesses_get_no_extra_tries_and_one_winner` cover sequential/concurrent checks. Redis counts each attempt atomically before comparison. Twilio enforces its own five-check limit. |
| Constant-time comparison | Local code hashes use `subtle::ConstantTimeEq`; no plaintext code comparison. Twilio compares production SMS codes internally. |
| Reject reused/replaced codes | `a_code_works_once`, `a_new_code_replaces_the_old_one`, and the resend/in-flight-check regression. Consumption atomically matches the hash and expiry before deleting the challenge. |
| Rate limits per address and IP, returning 429 | `rate_limits` covers 1 send/address/minute, 5/address/hour, 300 sends/IP/hour, 600 checks/IP/hour and 20 checks/address/day. Route tests also cover local and provider 429s. |
| E.164 phones | `Dest::phone` accepts only canonical `+` followed by 2–15 digits, with no leading zero. `address_rules` rejects unformatted/local numbers; the client formats them before calling the API. |
| Postmark / Twilio Verify integration | Mock-provider tests check destinations, payloads, provider credentials, code approval, 404/429 responses and outages. Unexpected redirects fail closed rather than forwarding credentials/codes. |
| Successful sign-in creates the right session | `sign_in_routes` checks web HttpOnly cookie delivery, native bearer-token delivery, verified email/phone persistence, account reuse, and no implicit roles. The inherited session/role matrix tests run in the same suite. |
| Providers or Redis unavailable | Failure tests check 503 behavior; a failed resend preserves the previously delivered code. No send happens when Redis cannot enforce abuse limits. |
| Email and SMS actually arrive and work | **Email passed live (2026-11-21).** Postmark accepted an email to a user-provided lsu.edu recipient (202); the user supplied the received code; verification and profile lookup returned 200, code reuse returned 401, logout returned 204 and the logged-out session returned 401. Secure/HttpOnly/SameSite=Lax/14-day cookie, no token in JSON and no-store checks passed. **SMS remains pending** because its configuration is incomplete. |

## Local verification

Result: **29 tests passed**, Clippy passed with warnings denied, the release
library build passed, and `git diff --check` was clean.

Run the dev Postgres/Redis services, then `cargo test -p auth` with `DATABASE_URL`
and `REDIS_URL` set. Also run:

```
cargo clippy -p auth --all-targets -- -D warnings
cargo check -p auth --release --lib
```

The concurrency regression was run against the original unconditional deletion
and failed with “old challenge consumed a newer code”; it passes after binding
consumption to the hash and expiry that were checked.

## Remaining live checks

Postmark token/sender settings are working. Local provider settings are now in
ignored `server/.env` with owner-only permissions (`0600`); the tracked
`.env.example` contains blank placeholders. Git-ignore and tracked-file credential
checks passed. For SMS,
complete the Twilio account SID + auth token + Verify service SID. Confirm the Twilio service has a six-digit code length, a
ten-minute validity period and the required destination-country permissions.

The email delivery/sign-in/reuse/logout flow passed live. Repeat with a
user-selected E.164 phone once SMS is configured.
Observe actual SMS expiry / attempt-limit behavior in the configured service.
Do not use a mock or Postmark's non-delivering `POSTMARK_API_TEST` token as evidence
that a message arrived. No live delivery latency threshold is specified for PR 2.

Twilio documents [ten-minute default validity](https://www.twilio.com/docs/verify/api/rate-limits-and-timeouts),
[five check attempts](https://www.twilio.com/docs/api/errors/60202), and
[404 after expiry/approval/exhaustion](https://www.twilio.com/docs/verify/api/verification-check).
Those are provider guarantees to verify in the real configured service, rather
than properties independently enforced by this crate for production SMS.

Live Postmark notes (2026-11-21): Gmail delivery remains blocked by HTTP 422 /
ErrorCode 412 while account approval restricts the lsu.edu sender to lsu.edu
recipients. The first LSU attempt coincided with a TLS timeout and no outbound
record/challenge; a retry was accepted and the user-confirmed email flow above
passed. No application timeout or production code changes were made for this
live test. The temporary test session was revoked after verification.
