# OAuth sign-in (PR 3)

This branch is stacked on PR 2 (`ricky/auth-email-codes` at `9e58acf`). Keep both
`AuthState.codes` and `AuthState.oauth`. OAuth reuses PR 2's address validation,
account lookup, client-IP handling and session delivery, so both sign-in methods
use the same accounts and cookie/native-token behavior.

Google, GitHub and Apple use the existing 14-day sessions and `users.email`.
Only a provider-verified email can create or link an account. A new account has
no roles. A missing/unverified email is rejected; users can sign in by another
method. Provider subjects and access/refresh tokens are not persisted. Account
matching is by verified email on every sign-in, so a changed provider email can
select a different account. Apple private-relay addresses are separate addresses.
No migration is added (judge rooms, PR 4, take `0002`).

## Server setup

Set `AuthState.oauth = auth::OAuth::from_env()?` once at startup. A provider with
no variables is disabled (503); partially configured providers fail startup.
`OAuth::default()` disables all providers, useful in existing stand-in routers.

| Provider | Environment variables |
|---|---|
| Google | `OAUTH_GOOGLE_CLIENT_ID`, `OAUTH_GOOGLE_CLIENT_SECRET` |
| GitHub | `OAUTH_GITHUB_CLIENT_ID`, `OAUTH_GITHUB_CLIENT_SECRET` |
| Apple | `OAUTH_APPLE_CLIENT_ID` (Services ID), `OAUTH_APPLE_TEAM_ID`, `OAUTH_APPLE_KEY_ID`, `OAUTH_APPLE_PRIVATE_KEY` (P-256 PEM with real newlines) |

Keep secrets in the deployment's secrets store. Apple client-secret JWTs are
signed with ES256 per exchange and expire after five minutes. Provider ID tokens
must use RS256 and pass signature, issuer, audience, expiry and nonce checks.
Public keys are cached for five minutes; a newly rotated key can be rejected
until that cache expires. Requests time out after ten seconds and never follow
redirects carrying credentials. TLS roots are bundled for Kenny's scratch image.

The production domain is `geauxhack.org` (confirmed by Ricky on 2026-11-21).
Register exactly `https://app.geauxhack.org/auth/callback` with each provider
(or the deployed `AuthState.origin` plus `/auth/callback`). Use the Google web
OAuth client for this server-mediated flow on both web and native. Native launches
the returned URL in the system browser, with the same HTTPS Universal/App Link.
The backend holds the client secrets.

The HTTPS proxy must route **POST `/auth/callback` to the API** (Apple form_post),
and **GET `/auth/callback` to the client**. Do not log callback query strings or
bodies, completion bodies, provider tokens, or session responses. The client
callback page must also send `Cache-Control: no-store` and
`Referrer-Policy: no-referrer`, avoid third-party scripts/resources, and strip the
query with `history.replaceState` immediately after reading it.

The proxy must append the real client IP to `X-Forwarded-For`; the API must be
internal to that proxy. Direct connections need
`into_make_service_with_connect_info::<SocketAddr>()`. Starts are limited to 300
per IP per hour (shared venue Wi-Fi); pending flows expire after ten minutes.
Redis failure returns 503; missing client IP returns 500. `auth-dev serve` loads
the same variables; `AUTH_DEV_ORIGIN` can point it at an HTTPS development proxy
with an allowed callback. Apple requires a registered HTTPS domain.

For local browser tests, Safari requires HTTPS to retain the Secure session
cookie. On plain `http://127.0.0.1`, a provider exchange can return 200 and issue
a cookie while the next `/api/v1/me` returns 401 because Safari did not store/send
it. Use trusted HTTPS for Safari, or Chrome/the Codex browser for the current
loopback HTTP harness. Keep the production cookie flags unchanged.

## Client handshake (Josh)

1. Generate a fresh random 32-byte verifier, base64url without padding (43
   characters). Save it locally for this attempt. Compute
   `codeChallenge = base64url(SHA256(ASCII(verifier)))` without padding.
2. POST `/api/v1/auth/oauth/{google|github|apple}/start` with
   `{"codeChallenge":"..."}`. Save the returned `state` and chosen provider next
   to the verifier **before** navigating to the returned `url`. The browser
   supplies the web `Origin`; native calls through its API client.
3. On `/auth/callback`, read `code` and `state`, verify the state matches a local
   pending attempt, and remove the query. Use the provider saved in step 2.
   Handle `error` as cancellation and clear the local attempt. Apple POSTs to
   the API first; its 303 response relays code/state to this same client route.
4. POST `/api/v1/auth/oauth/{provider}/complete` with
   `{"state":"...","code":"...","codeVerifier":"..."}`. The API atomically
   consumes the pending flow, then exchanges the code. Google/GitHub enforce
   S256 PKCE too. Apple does not advertise PKCE: its signed ID-token nonce binds
   the provider response, and our completion endpoint still enforces the verifier.
5. Web (our `Origin` at both start and completion) receives `{id, roles, exp}`
   and the Secure/HttpOnly session cookie. Native receives that profile plus
   `token` in the JSON body, for secure storage. No session token goes in a URL.

Clear the local attempt after completion. 401 means invalid, expired or used
flow/provider response; 400 means malformed input; 429 means rate limited;
503 means a provider/Redis/Postgres/configuration failure. Once completion has
consumed the flow, failures require a **new start**, not replaying the old code.
OAuth responses, including errors, carry `no-store` and `no-referrer`.

## Validation

`cargo test -p auth` with the dev Postgres/Redis tests provider exchanges against
local HTTP servers, including email-code sign-in followed by OAuth for the same
account/roles, signed Google/Apple fixtures, Apple client-secret
signatures, PKCE forwarding, nonce/audience/issuer/time checks, callback replay,
concurrent completion, verified-email linking, web/native delivery, rate limits,
Redis failure and provider failures. The PEM files under `src/oauth/` are generated
test-only keys, never deployment credentials.

Live Google and GitHub tests on 2026-11-21 completed their provider exchanges,
issued web session cookies and subsequently returned `/api/v1/me` 200 with the
cookie sent. Earlier Safari attempts over local HTTP omitted the Secure cookie
and returned 401 after successful exchange. The successful Google and GitHub
flows used the same stored verified email and account (checked without recording
personal details). Live browser logout returned 204 and the next profile request
returned 401; its session was confirmed revoked in Postgres and cached with a
positive TTL in Redis. Automated tests additionally resend the original cookie
and native bearer token after logout, proving revocation independently of cookie
clearing.

## Acceptance status and deferred blockers

The original sign-in acceptance criterion requires Google, GitHub and Apple to
work on web, iOS and Android and return through the HTTPS Universal/App Link.
The backend is implemented; that complete end-to-end criterion remains open.

| Requirement | Current evidence | Remaining work / owner |
|---|---|---|
| Google and GitHub web sign-in | Live local exchanges and authenticated profile requests returned 200 for both providers; verified email linked both to one account. | Local check complete. Recheck on deployed HTTPS, including Safari. Auth/infra. |
| Apple sign-in | Backend exchange, signed identity validation, client-secret signing and POST relay are tested with fixtures. | **Deferred by Ricky on 2026-11-21.** Obtain active Apple Developer Program membership (normally USD 99/year, or an approved fee waiver); team must decide the legal owner and Account Holder. Then configure App ID, Services ID, key and HTTPS return URL and test live. Team/ops/auth. |
| Native OAuth on iOS and Android | Backend native token responses are tested. | Implement system-browser launch and callback handling in the Capacitor client, then test each provider on physical iPhone and Android devices. Josh/auth. |
| Universal/App Link callback | Contract specifies `https://app.geauxhack.org/auth/callback` and documents verifier/state handling. | DNS and HTTPS for `app.geauxhack.org` are live (droplet `161.35.242.244`, Caddy with a Let's Encrypt certificate, 2026-11-21). Host AASA/assetlinks, configure native associations, serve the GET callback UI and route Apple POST callbacks to the API. Josh/infra. |
| Verified-email account linking and session delivery | Live Google/GitHub account reuse passed. Tests also cover code-to-GitHub role reuse, Google/Apple account reuse, web cookie flags and native tokens. Provider tokens are not persisted. | Backend check complete. Wire the production client to the documented contract; native secure storage and offline startup are tracked in PR 5. |
| Logout and immediate revocation | Live logout 204 → profile 401, Postgres revocation and Redis cache confirmed. Automated OAuth flows check original cookie/bearer rejection after logout and cross-site logout 403. | Local/backend check complete. |
| OAuth security and authorization | Automated tests cover verifier/state/provider/Origin binding, single use/concurrent completion, signed identity claims, failure handling, abuse limits and route × role policies. | No additional backend implementation gap identified by this acceptance audit. Apple uses nonce plus verifier-bound completion rather than provider-side PKCE, as documented above. |

Apple remains disabled while its environment variables are absent. Do not mark
Apple or the full web/iOS/Android criterion complete from fixture tests alone.
PR 2 owns email/SMS delivery acceptance; judge-room timing/access and general
offline-startup acceptance belong to their respective PRs.

`cargo test -p auth`, Clippy (`--all-targets -- -D warnings`) and the release library
check (`--release --lib`) pass. OAuth formatting uses the existing compact layout:

```sh
rustfmt --edition 2021 --config max_width=120,use_small_heuristics=Max --check auth/src/oauth.rs
```

Provider references: [Google OIDC](https://developers.google.com/identity/openid-connect),
[GitHub authorization code + PKCE](https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/authorizing-oauth-apps),
[Apple authorization](https://developer.apple.com/documentation/signinwithapplerestapi/request-an-authorization-to-the-sign-in-with-apple-server.),
[Apple token exchange](https://developer.apple.com/documentation/signinwithapplerestapi/generate-and-validate-tokens).
