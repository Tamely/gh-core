//! PR 2: sign-in by email or SMS code. Codes and limits live in the dev Redis;
//! every test uses fresh random addresses and IPs, so runs don't collide.

mod common;

use std::sync::{
    atomic::{AtomicI64, Ordering::SeqCst},
    Arc,
};

use auth::{CodeError, Dest, CODE_SECS, MAX_TRIES};
use common::*;
use fred::prelude::{HashesInterface, KeysInterface};
use serde_json::Value;

fn email() -> Dest {
    email_as("")
}

/// A fresh email starting with `prefix` (the fake fails on some prefixes).
fn email_as(prefix: &str) -> Dest {
    Dest::email(&format!("{prefix}{}@example.com", random())).unwrap()
}

/// A fresh US number in area code `area` (the fake rejects 500, a landline).
fn phone_in(area: u16) -> String {
    format!("+1{area}{:07}", u64::from_str_radix(&random(), 16).unwrap() % 10_000_000)
}

fn phone() -> String {
    phone_in(225)
}

fn ip() -> String {
    format!("test-{}", random())
}

/// Any code other than `code`.
fn wrong(code: &str) -> String {
    format!("{:06}", (code.parse::<u32>().unwrap() + 1) % 1_000_000)
}

#[tokio::test]
async fn a_code_works_once() {
    let (fake, redis) = (fake_provider().await, redis().await);
    let codes = fake_codes(&fake);
    let (to, ip) = (email(), ip());
    codes.send(&redis, &to, &ip, NOW).await.unwrap();
    let code = last_code(&fake);
    assert_eq!(code.len(), 6);
    assert!(codes.check(&redis, &to, &code, &ip, NOW + 1).await.unwrap());
    assert!(!codes.check(&redis, &to, &code, &ip, NOW + 2).await.unwrap(), "second use");
}

#[tokio::test]
async fn five_tries_per_code() {
    let (fake, redis) = (fake_provider().await, redis().await);
    let codes = fake_codes(&fake);
    // (wrong guesses first, then the right code: does it still work?)
    for (wrong_guesses, works) in [(0, true), (1, true), (4, true), (5, false), (6, false)] {
        let (to, ip) = (email(), ip());
        codes.send(&redis, &to, &ip, NOW).await.unwrap();
        let code = last_code(&fake);
        for _ in 0..wrong_guesses {
            assert!(!codes.check(&redis, &to, &wrong(&code), &ip, NOW).await.unwrap());
        }
        let got = codes.check(&redis, &to, &code, &ip, NOW).await.unwrap();
        assert_eq!(got, works, "right code after {wrong_guesses} wrong guesses (limit {MAX_TRIES})");
    }
}

#[tokio::test]
async fn parallel_guesses_get_no_extra_tries_and_one_winner() {
    let (fake, redis) = (fake_provider().await, redis().await);
    let codes = Arc::new(fake_codes(&fake));
    let check_all = |to: Dest, guess: String, n: usize| {
        let mut set = tokio::task::JoinSet::new();
        for _ in 0..n {
            let (codes, redis, to, guess) = (codes.clone(), redis.clone(), to.clone(), guess.clone());
            set.spawn(async move { codes.check(&redis, &to, &guess, &ip(), NOW).await.unwrap() });
        }
        set.join_all()
    };

    // 10 right guesses at once: exactly one signs in.
    let to = email();
    codes.send(&redis, &to, &ip(), NOW).await.unwrap();
    let wins = check_all(to, last_code(&fake), 10).await;
    assert_eq!(wins.iter().filter(|&&w| w).count(), 1, "{wins:?}");

    // 10 wrong guesses at once use up all 5 tries; the right code is then dead.
    let to = email();
    codes.send(&redis, &to, &ip(), NOW).await.unwrap();
    let code = last_code(&fake);
    check_all(to.clone(), wrong(&code), 10).await;
    assert!(!codes.check(&redis, &to, &code, &ip(), NOW).await.unwrap());
}

#[tokio::test]
async fn codes_expire_after_10_minutes() {
    let (fake, redis) = (fake_provider().await, redis().await);
    let codes = fake_codes(&fake);
    for (age, works) in [(0, true), (CODE_SECS - 1, true), (CODE_SECS, false), (CODE_SECS * 6, false)] {
        let (to, ip) = (email(), ip());
        codes.send(&redis, &to, &ip, NOW).await.unwrap();
        let got = codes.check(&redis, &to, &last_code(&fake), &ip, NOW + age).await.unwrap();
        assert_eq!(got, works, "code {age}s old");
    }
}

#[tokio::test]
async fn a_new_code_replaces_the_old_one() {
    let (fake, redis) = (fake_provider().await, redis().await);
    let codes = fake_codes(&fake);
    let (to, ip) = (email(), ip());
    codes.send(&redis, &to, &ip, NOW).await.unwrap();
    let old = last_code(&fake);
    codes.send(&redis, &to, &ip, NOW + 60).await.unwrap();
    let new = last_code(&fake);
    if old != new {
        assert!(!codes.check(&redis, &to, &old, &ip, NOW + 61).await.unwrap(), "old code");
    }
    assert!(codes.check(&redis, &to, &new, &ip, NOW + 62).await.unwrap(), "new code");
}

#[tokio::test]
async fn redis_keeps_a_hash_not_the_code() {
    let (fake, redis) = (fake_provider().await, redis().await);
    let codes = fake_codes(&fake);
    let to = email();
    codes.send(&redis, &to, &ip(), NOW).await.unwrap();
    let code = last_code(&fake);
    let Dest::Email(addr) = &to else { unreachable!() };
    let stored: std::collections::HashMap<String, String> =
        redis.hgetall(format!("gh:code:email:{addr}")).await.unwrap();
    assert_eq!(stored["h"].len(), 64, "a hex SHA-256: {stored:?}");
    assert!(stored.values().all(|v| !v.contains(&code)), "{stored:?} contains {code}");
    assert_eq!(stored["exp"], (NOW + CODE_SECS).to_string());
    let ttl: i64 = redis.ttl(format!("gh:code:email:{addr}")).await.unwrap();
    assert!((1..=CODE_SECS).contains(&ttl), "code hash must expire from Redis in ten minutes");
}

#[tokio::test]
async fn rate_limits() {
    let (fake, redis) = (fake_provider().await, redis().await);
    let codes = fake_codes(&fake);
    let send = |to: Dest, ip: String, at: i64| {
        let (codes, redis) = (&codes, &redis);
        async move { codes.send(redis, &to, &ip, NOW + at).await }
    };
    let too_many = Err(CodeError::TooMany);

    // One code a minute per address.
    let to = email();
    assert_eq!(send(to.clone(), ip(), 0).await, Ok(()));
    assert_eq!(send(to.clone(), ip(), 59).await, too_many);
    assert_eq!(send(to.clone(), ip(), 60).await, Ok(()));

    // Five an hour per address. NOW is the start of an hour.
    let to = email();
    for minute in 0..5 {
        assert_eq!(send(to.clone(), ip(), minute * 60).await, Ok(()), "send {minute}");
    }
    assert_eq!(send(to.clone(), ip(), 5 * 60).await, too_many);
    assert_eq!(send(to.clone(), ip(), 3600).await, Ok(()), "next hour");

    // 300 sends an hour per IP, to any addresses (a whole venue can share one IP).
    let one_ip = ip();
    for n in 0..300 {
        assert_eq!(send(email(), one_ip.clone(), 0).await, Ok(()), "send {n}");
    }
    assert_eq!(send(email(), one_ip.clone(), 0).await, too_many);

    // 600 guesses an hour per IP, on any addresses.
    let one_ip = ip();
    for _ in 0..600 {
        assert_eq!(codes.check(&redis, &email(), "123456", &one_ip, NOW).await, Ok(false));
    }
    assert_eq!(codes.check(&redis, &email(), "123456", &one_ip, NOW).await, Err(CodeError::TooMany));

    // 20 guesses a day per address, from any IPs and across new codes.
    let (to, day) = (email(), NOW - NOW % 86400);
    for n in 0..20 {
        assert_eq!(codes.check(&redis, &to, "123456", &ip(), day + n * 3600).await, Ok(false), "guess {n}");
    }
    assert_eq!(codes.check(&redis, &to, "123456", &ip(), day + 23 * 3600).await, Err(CodeError::TooMany));
    assert_eq!(codes.check(&redis, &to, "123456", &ip(), day + 86400).await, Ok(false), "next day");
}

#[tokio::test]
async fn provider_answers() {
    let (fake, redis) = (fake_provider().await, redis().await);
    let codes = fake_codes(&fake);
    let send = |to: Dest| {
        let (codes, redis) = (&codes, &redis);
        async move { codes.send(redis, &to, &ip(), NOW).await }
    };
    // The address can never work: 400, so the user fixes it rather than retrying.
    assert_eq!(send(email_as("bounced-")).await, Err(CodeError::BadAddress));
    let landline = Dest::phone(&phone_in(500)).unwrap();
    assert_eq!(send(landline).await, Err(CodeError::BadAddress));
    // Our own setup is wrong: 503, not the user's fault.
    assert_eq!(send(email_as("unconfirmed-")).await, Err(CodeError::Unavailable));
    // Postmark's rate limit: 429, like Twilio's.
    assert_eq!(send(email_as("busy-")).await, Err(CodeError::TooMany));
    // Twilio answered 200 but we couldn't read it (it may have approved): 503, not "wrong code".
    let to = Dest::phone(&phone()).unwrap();
    assert_eq!(codes.check(&redis, &to, "999999", &ip(), NOW).await, Err(CodeError::Unavailable));
}

#[tokio::test]
async fn postmark_request() {
    let (fake, redis) = (fake_provider().await, redis().await);
    let to = email();
    fake_codes(&fake).send(&redis, &to, &ip(), NOW).await.unwrap();
    let seen = fake.seen.lock().unwrap().last().cloned().unwrap();
    assert_eq!(seen.path, "/email");
    assert_eq!(seen.headers["x-postmark-server-token"], "pm-token");
    let body: Value = serde_json::from_str(&seen.body).unwrap();
    let Dest::Email(addr) = to else { unreachable!() };
    assert_eq!(body["To"], addr.as_str());
    assert_eq!(body["From"], "codes@geauxhack.org");
    assert_eq!(body["MessageStream"], "outbound");
}

#[tokio::test]
async fn sms_goes_through_twilio_verify() {
    let (fake, redis) = (fake_provider().await, redis().await);
    let codes = fake_codes(&fake);
    let (number, ip) = (phone(), ip());
    let to = Dest::phone(&number).unwrap();
    codes.send(&redis, &to, &ip, NOW).await.unwrap();
    let seen = fake.seen.lock().unwrap().last().cloned().unwrap();
    assert_eq!(seen.path, "/v2/Services/VA123/Verifications");
    assert_eq!(seen.body, format!("To=%2B{}&Channel=sms", &number[1..]));
    // Basic auth with the account SID and auth token: base64("AC123:tw-token").
    assert_eq!(seen.headers["authorization"], "Basic QUMxMjM6dHctdG9rZW4=");

    // The fake approves only 123456; Twilio keeps its own tries and expiry.
    assert!(!codes.check(&redis, &to, "000000", &ip, NOW).await.unwrap());
    assert!(codes.check(&redis, &to, "123456", &ip, NOW).await.unwrap());
    let seen = fake.seen.lock().unwrap().last().cloned().unwrap();
    assert_eq!(seen.path, "/v2/Services/VA123/VerificationCheck");
}

/// Provider down: 503, and a failed resend leaves the code already delivered working.
#[tokio::test]
async fn provider_down_is_503() {
    let (fake, redis) = (fake_provider().await, redis().await);
    let (to, ip) = (email(), ip());
    fake_codes(&fake).send(&redis, &to, &ip, NOW).await.unwrap();
    let code = last_code(&fake);
    let dead = Fake { url: "http://127.0.0.1:1".into(), seen: fake.seen.clone() };
    let codes = fake_codes(&dead);
    assert_eq!(codes.send(&redis, &to, &ip, NOW + 60).await, Err(CodeError::Unavailable));
    assert!(fake_codes(&fake).check(&redis, &to, &code, &ip, NOW + 61).await.unwrap(), "the code already delivered");
    let phone = Dest::phone(&phone()).unwrap();
    assert_eq!(codes.send(&redis, &phone, &ip, NOW).await, Err(CodeError::Unavailable));
    assert_eq!(codes.check(&redis, &phone, "123456", &ip, NOW).await, Err(CodeError::Unavailable));
}

#[tokio::test]
async fn provider_redirects_do_not_forward_credentials_or_codes() {
    use axum::{http::{header, StatusCode}, Router};
    let destination = fake_provider().await;
    let target = format!("{}/email", destination.url);
    let app = Router::new().fallback(move || {
        let target = target.clone();
        async move { (StatusCode::TEMPORARY_REDIRECT, [(header::LOCATION, target)]) }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let codes = fake_codes(&Fake { url, seen: destination.seen.clone() });
    let redis = redis().await;
    for to in [email(), Dest::phone(&phone()).unwrap()] {
        assert_eq!(codes.send(&redis, &to, &ip(), NOW).await, Err(CodeError::Unavailable));
    }
    assert!(destination.seen.lock().unwrap().is_empty(), "redirect received provider credentials");
}

#[tokio::test]
async fn redis_down_cannot_send_or_verify_codes() {
    let fake = fake_provider().await;
    let codes = fake_codes(&fake);
    let redis = dead_redis().await;
    let to = email();
    assert_eq!(codes.send(&redis, &to, &ip(), NOW).await, Err(CodeError::Unavailable));
    assert_eq!(codes.check(&redis, &to, "123456", &ip(), NOW).await, Err(CodeError::Unavailable));
    assert!(fake.seen.lock().unwrap().is_empty(), "a code must not be sent without abuse limits");
}

#[test]
fn address_rules() {
    let email = |s| Dest::email(s).map(|d| format!("{d:?}"));
    assert_eq!(email("  Ricky@LSU.edu "), Some(r#"Email("ricky@lsu.edu")"#.into()));
    assert_eq!(email("o'neil+gh@cs.lsu.edu"), Some(r#"Email("o'neil+gh@cs.lsu.edu")"#.into()));
    for bad in [
        "", "ricky", "@lsu.edu", "ricky@", "ricky@lsu", "ricky@.lsu.edu", "ricky@lsu.edu.", "a@b@c.d", "ric ky@lsu.edu",
        // Postmark would read these as a display name, a list, or a comment.
        "x<attacker@evil.com>", "Ricky <ricky@lsu.edu>", "ricky<x@lsu.edu", "victim@lsu.edu,attacker@evil.com", "a@b.c,d", "\"a\"@b.c", "a(b)@c.d", "a@b..c",
        "ricky@lsu.edu;x", "rícky@lsu.edu",
    ] {
        assert_eq!(email(bad), None, "{bad:?}");
    }
    assert_eq!(Dest::phone("+12255551234"), Some(Dest::Phone("+12255551234".into())));
    for bad in ["", "12255551234", "+", "+1", "+02255551234", "+1 225 555 1234", "+1-225-555-1234", "+1234567890123456"] {
        assert_eq!(Dest::phone(bad), None, "{bad:?}");
    }
}

static CLOCK: AtomicI64 = AtomicI64::new(NOW);

fn clock() -> i64 {
    CLOCK.load(SeqCst)
}

/// The two routes end to end: cookie for our web page, token for everything else.
#[tokio::test]
async fn sign_in_routes() {
    let fake = fake_provider().await;
    let db = db("auth_codes_test").await;
    let state = state_with(db.clone(), redis().await, clock, fake_codes(&fake));
    state.sessions.reload(NOW).await.unwrap();
    let app = app(state, axum::Router::new());
    let ip = ip();
    let json = [("content-type", "application/json"), ("x-forwarded-for", &ip)];
    let web = [json[0], json[1], ("origin", ORIGIN)];
    let addr = format!("{}@Example.com", random());
    let ask = format!(r#"{{"email": "  {addr} "}}"#);
    let verify = |code: &str| format!(r#"{{"email": "{addr}", "code": "{code}"}}"#);

    // Web: the cookie only; the token never reaches page scripts.
    assert_eq!(call_body(&app, "POST", "/api/v1/auth/code", &json, &ask).await.status, 202);
    let code = last_code(&fake);
    assert_eq!(call_body(&app, "POST", "/api/v1/auth/code/verify", &web, &verify(&wrong(&code))).await.status, 401);
    let res = call_body(&app, "POST", "/api/v1/auth/code/verify", &web, &verify(&code)).await;
    assert_eq!(res.status, 200, "{}", res.body);
    assert_eq!(res.headers["cache-control"], "no-store");
    let cookie = res.headers["set-cookie"].to_str().unwrap().split(';').next().unwrap().to_string();
    assert!(cookie.starts_with("__Host-session="), "{cookie}");
    let body: Value = serde_json::from_str(&res.body).unwrap();
    assert_eq!(body.get("token"), None);
    assert_eq!(body["roles"], serde_json::json!([]), "new accounts have no roles");
    let me = call(&app, "GET", "/api/v1/me", &[("cookie", &cookie)]).await;
    assert_eq!(me.status, 200);
    let id = body["id"].as_str().unwrap().to_string();
    assert!(me.body.contains(&id));
    let stored: String = sqlx::query_scalar("SELECT email FROM users WHERE id = $1::uuid").bind(&id).fetch_one(&db).await.unwrap();
    assert_eq!(stored, addr.to_lowercase());

    // Native (no Origin), a minute later: token in the body, same account.
    CLOCK.store(NOW + 60, SeqCst);
    assert_eq!(call_body(&app, "POST", "/api/v1/auth/code", &json, &ask).await.status, 202);
    let res = call_body(&app, "POST", "/api/v1/auth/code/verify", &json, &verify(&last_code(&fake))).await;
    assert_eq!(res.status, 200);
    assert!(!res.headers.contains_key("set-cookie"));
    let body: Value = serde_json::from_str(&res.body).unwrap();
    assert_eq!(body["id"], id.as_str(), "same email, same account");
    let bearer = format!("Bearer {}", body["token"].as_str().unwrap());
    assert_eq!(call(&app, "GET", "/api/v1/me", &[("authorization", &bearer)]).await.status, 200);

    // Another site's page: no cookie (and the browser hides the body from it).
    CLOCK.store(NOW + 120, SeqCst);
    call_body(&app, "POST", "/api/v1/auth/code", &json, &ask).await;
    let evil = [json[0], json[1], ("origin", "https://evil.example")];
    let res = call_body(&app, "POST", "/api/v1/auth/code/verify", &evil, &verify(&last_code(&fake))).await;
    assert_eq!(res.status, 200);
    assert!(!res.headers.contains_key("set-cookie"));

    // Phone, through the Twilio fake: a new account with only a phone.
    let number = phone();
    let phone_body = |code: &str| format!(r#"{{"phone": "{number}", "code": "{code}"}}"#);
    let ask_phone = format!(r#"{{"phone": "{number}"}}"#);
    assert_eq!(call_body(&app, "POST", "/api/v1/auth/code", &json, &ask_phone).await.status, 202);
    let res = call_body(&app, "POST", "/api/v1/auth/code/verify", &json, &phone_body("123456")).await;
    assert_eq!(res.status, 200, "{}", res.body);
    let body: Value = serde_json::from_str(&res.body).unwrap();
    let (stored_email, stored_phone): (Option<String>, String) =
        sqlx::query_as("SELECT email, phone_e164 FROM users WHERE id = $1::uuid")
            .bind(body["id"].as_str().unwrap())
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!((stored_email, stored_phone), (None, number.clone()));

    // Twilio deletes expired/used/exhausted verifications (404), or returns
    // 429 when its five-check limit is reached. Neither may issue a session.
    for (code, status) in [("999998", 401), ("999997", 429), ("999999", 503)] {
        let res = call_body(&app, "POST", "/api/v1/auth/code/verify", &json, &phone_body(code)).await;
        assert_eq!(res.status, status);
        assert!(!res.headers.contains_key("set-cookie"));
        assert!(!res.body.contains("token"));
    }

    // Limits and bad requests.
    assert_eq!(call_body(&app, "POST", "/api/v1/auth/code", &json, &ask_phone).await.status, 429, "same minute");
    for bad in [
        r#"{"email": "not-an-email"}"#,
        r#"{"phone": "2255551234"}"#,
        r#"{}"#,
        &format!(r#"{{"email": "{addr}", "phone": "{number}"}}"#),
    ] {
        assert_eq!(call_body(&app, "POST", "/api/v1/auth/code", &json, bad).await.status, 400, "{bad}");
    }
    let no_code = format!(r#"{{"email": "{addr}"}}"#);
    assert_eq!(call_body(&app, "POST", "/api/v1/auth/code/verify", &json, &no_code).await.status, 400);
    let bounced = format!(r#"{{"email": "bounced-{}@example.com"}}"#, random());
    assert_eq!(call_body(&app, "POST", "/api/v1/auth/code", &json, &bounced).await.status, 400, "Postmark refused it");

    // A server that can't see client IPs fails loudly instead of sharing one limit.
    let no_ip = [("content-type", "application/json")];
    assert_eq!(call_body(&app, "POST", "/api/v1/auth/code", &no_ip, &ask).await.status, 500);
    assert_eq!(call_body(&app, "POST", "/api/v1/auth/code/verify", &no_ip, &verify("123456")).await.status, 500);
}
