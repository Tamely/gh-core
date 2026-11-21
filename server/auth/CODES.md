# Sign-in codes (PR 2): acceptance check

PR 2 covers sign-in by email or SMS code. OAuth is PR 3, judge rooms are PR 4,
and the client side (secure storage, offline startup) is PR 5.

| Requirement | Evidence (`tests/codes.rs` unless noted) | Status |
|---|---|---|
| Six digits from a secure random source | `new_code` uses OS randomness with rejection sampling (no modulo bias). Twilio Verify makes production SMS codes; its service must be set to six digits. | Done |
| Only a hash is stored | `redis_keeps_a_hash_not_the_code` checks what Redis holds and its TTL. Release builds can't log codes (`Sender::Log` is debug-only). | Done |
| Expires after 10 minutes | `codes_expire_after_10_minutes`: the exact boundary, with a fake clock. | Done |
| At most 5 attempts; the 6th is rejected | `five_tries_per_code`, `parallel_guesses_get_no_extra_tries_and_one_winner`. A try is counted in Redis before comparing. | Done |
| Constant-time comparison | Hashes compared with `subtle::ConstantTimeEq`. | Done |
| A reused or replaced code is rejected | `a_code_works_once`, `a_new_code_replaces_the_old_one`, and `an_in_flight_check_cannot_consume_a_resent_code` (in `src/codes.rs`). | Done |
| Rate limits per address and per IP return 429 | `rate_limits`: 1 send per address per minute, 5 per hour, 300 sends and 600 checks per IP per hour, 20 checks per address per day. Provider 429s map to 429 too (`provider_answers`). | Done |
| Phones in E.164 | `address_rules`. The client formats numbers before calling the API. | Done |
| Twilio Verify for SMS, Postmark for email | `postmark_request`, `sms_goes_through_twilio_verify`, `provider_answers`, `provider_down_is_503`, `provider_redirects_do_not_forward_credentials_or_codes` (against local fake providers). | Done |
| Sign-in gives the right session | `sign_in_routes`: web gets the HttpOnly cookie, native gets `token` in the body, the account is created once and reused, new accounts have no roles. | Done |
| Redis or a provider down | `redis_down_cannot_send_or_verify_codes`, `provider_down_is_503`: 503, never a send without rate limits. A failed resend keeps the earlier code working. | Done |
| Email codes arrive and work | Live, 2026-11-21, through Postmark to an lsu.edu address: send 202, verify 200, `/me` 200, reuse 401, logout 204, then 401. Cookie flags and `no-store` checked. | Done |
| SMS codes arrive and work | Needs the Twilio Verify account SID, auth token and service SID. Then confirm the service uses 6 digits, 10 minutes and US sending, and try a real phone. | **Open (ops)** |

Postmark only delivers to lsu.edu addresses until the account is approved (other
recipients get 422 / ErrorCode 412).

## Running it

Start the dev databases (`server/docker-compose.yml`), then with `DATABASE_URL`
and `REDIS_URL` set:

```
cargo test -p auth
cargo clippy -p auth --all-targets -- -D warnings
```

Provider settings go in the git-ignored `server/.env` (copy `server/.env.example`).
With a channel's settings left out, `auth-dev serve` prints that channel's codes
instead of sending them. `POSTMARK_API_TEST` checks requests without delivering,
so it isn't evidence that an email arrived.

Twilio documents its own [10-minute validity](https://www.twilio.com/docs/verify/api/rate-limits-and-timeouts),
[five check attempts](https://www.twilio.com/docs/api/errors/60202) and
[404 once a code is used up](https://www.twilio.com/docs/verify/api/verification-check);
check them against the configured service.
