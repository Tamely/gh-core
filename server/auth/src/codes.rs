//! One-time sign-in codes by email or SMS.
//!
//! Our own codes (email, and SMS in dev): a random 6-digit code, of which Redis
//! keeps only a SHA-256 hash, for 10 minutes and 5 tries. A right code works once.
//! SMS in production: Twilio Verify makes, sends and checks the code itself.
//! Either way, rate limits per address and per IP are checked first.

use std::time::Duration;

use axum::http::StatusCode;
use fred::prelude::{Client, LuaInterface};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use subtle::ConstantTimeEq;

/// A code lasts 10 minutes.
pub const CODE_SECS: i64 = 10 * 60;
/// Wrong (or right) guesses allowed per code.
pub const MAX_TRIES: i64 = 5;

/// (name, window in seconds, max per window). Every send hits the three SEND
/// limits, every check the two CHECK limits.
// ponytail: per-IP limits sized for a venue where everyone shares one Wi-Fi IP;
// the per-address limits are the real guard. Allow-list the venue IP if check-in outgrows them.
const SEND_PER_IP: (&str, i64, i64) = ("send-ip", 3600, 300);
const SEND_PER_MINUTE: (&str, i64, i64) = ("send", 60, 1);
const SEND_PER_HOUR: (&str, i64, i64) = ("send", 3600, 5);
const CHECK_PER_IP: (&str, i64, i64) = ("check-ip", 3600, 600);
/// Guesses per address per day, across all its codes and IPs, so nobody can
/// keep guessing at one account (5 codes an hour x 5 tries would allow 600 a day).
const CHECK_PER_DAY: (&str, i64, i64) = ("check", 86400, 20);

/// Where a code goes. Built with `Dest::email` / `Dest::phone`, which validate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dest {
    Email(String),
    Phone(String),
}

impl Dest {
    /// Trimmed and lowercased, or `None` if it isn't shaped like an email.
    /// Plain ASCII `local@domain` only: no display names, lists, quotes or
    /// comments, which Postmark's `To` would read as another recipient.
    pub fn email(s: &str) -> Option<Self> {
        let s = s.trim().to_ascii_lowercase();
        let (local, domain) = s.split_once('@')?;
        let ok = s.len() <= 254
            && !local.is_empty()
            && local.chars().all(|c| c.is_ascii_alphanumeric() || ".!#$%&'*+/=?^_`{|}~-".contains(c))
            && domain.contains('.')
            && domain.split('.').all(|label| !label.is_empty() && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
        ok.then_some(Dest::Email(s))
    }

    /// E.164 only (`+12255551234`), the same rule as the `users` table.
    /// The app formats numbers before sending them.
    pub fn phone(s: &str) -> Option<Self> {
        let digits = s.strip_prefix('+')?;
        let ok = (2..=15).contains(&digits.len())
            && !digits.starts_with('0')
            && digits.bytes().all(|b| b.is_ascii_digit());
        ok.then(|| Dest::Phone(s.into()))
    }

    fn key(&self) -> String {
        match self {
            Dest::Email(e) => format!("email:{e}"),
            Dest::Phone(p) => format!("phone:{p}"),
        }
    }
}

/// Who delivers codes.
pub enum Sender {
    /// DEV ONLY: prints the code to stderr. Doesn't exist in release builds, so
    /// a production server can't be configured to log codes.
    #[cfg(debug_assertions)]
    Log,
    /// Email through Postmark. `api` is `https://api.postmarkapp.com`.
    Postmark { api: String, token: String, from: String },
    /// SMS through Twilio Verify. `api` is `https://verify.twilio.com`.
    TwilioVerify { api: String, account_sid: String, auth_token: String, service_sid: String },
}

#[derive(Debug, PartialEq, Eq)]
pub enum CodeError {
    /// A rate limit was hit (ours or the provider's): 429.
    TooMany,
    /// The provider refused the address for good (invalid or landline number,
    /// bounced email): 400, so the user fixes it instead of retrying.
    BadAddress,
    /// Redis or the provider didn't answer: 503.
    Unavailable,
}

impl CodeError {
    pub fn status(&self) -> StatusCode {
        match self {
            CodeError::TooMany => StatusCode::TOO_MANY_REQUESTS,
            CodeError::BadAddress => StatusCode::BAD_REQUEST,
            CodeError::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        }
    }
}

fn down<E>(_: E) -> CodeError {
    CodeError::Unavailable
}

pub struct Codes {
    email: Sender,
    sms: Sender,
    http: reqwest::Client,
}

impl Codes {
    pub fn new(email: Sender, sms: Sender) -> Self {
        assert!(!matches!(email, Sender::TwilioVerify { .. }), "Twilio Verify sends SMS, not email");
        assert!(!matches!(sms, Sender::Postmark { .. }), "Postmark sends email, not SMS");
        // Under Kenny's 3s request timeout, so a slow provider is a 503, not a cut-off request.
        let http = reqwest::Client::builder().timeout(Duration::from_secs(2))
            .redirect(reqwest::redirect::Policy::none()).build().expect("TLS backend loads");
        Self { email, sms, http }
    }

    /// The real senders, from POSTMARK_SERVER_TOKEN + POSTMARK_FROM (email) and
    /// TWILIO_ACCOUNT_SID + TWILIO_AUTH_TOKEN + TWILIO_VERIFY_SERVICE_SID (SMS).
    /// In debug builds a channel whose first variable is unset prints its codes instead.
    pub fn from_env() -> Result<Self, String> {
        let var = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
        let need = |name: &str| var(name).ok_or(format!("set {name}"));
        let or_log = |_name: &str| {
            #[cfg(debug_assertions)]
            return Ok::<_, String>(Sender::Log);
            #[cfg(not(debug_assertions))]
            Err(format!("set {_name}"))
        };
        let email = match var("POSTMARK_SERVER_TOKEN") {
            Some(token) => Sender::Postmark { api: "https://api.postmarkapp.com".into(), token, from: need("POSTMARK_FROM")? },
            None => or_log("POSTMARK_SERVER_TOKEN")?,
        };
        let sms = match var("TWILIO_ACCOUNT_SID") {
            Some(account_sid) => Sender::TwilioVerify {
                api: "https://verify.twilio.com".into(),
                account_sid,
                auth_token: need("TWILIO_AUTH_TOKEN")?,
                service_sid: need("TWILIO_VERIFY_SERVICE_SID")?,
            },
            None => or_log("TWILIO_ACCOUNT_SID")?,
        };
        Ok(Self::new(email, sms))
    }

    /// Sends a new code, replacing any earlier one for this address.
    pub async fn send(&self, redis: &Client, to: &Dest, ip: &str, now: i64) -> Result<(), CodeError> {
        let addr = to.key();
        limit(redis, SEND_PER_IP, ip, now).await?;
        limit(redis, SEND_PER_MINUTE, &addr, now).await?;
        limit(redis, SEND_PER_HOUR, &addr, now).await?;
        let sender = match to {
            Dest::Email(_) => &self.email,
            Dest::Phone(_) => &self.sms,
        };
        let code = new_code();
        match (sender, to) {
            (Sender::TwilioVerify { .. }, Dest::Phone(phone)) => {
                // Twilio makes, keeps and checks the code itself.
                return match self.twilio("Verifications", &[("To", phone), ("Channel", "sms")]).await?.0 {
                    201 => Ok(()),
                    // 400: invalid or landline number; 403: a country we don't send to.
                    400 | 403 => Err(CodeError::BadAddress),
                    _ => Err(CodeError::Unavailable),
                };
            }
            #[cfg(debug_assertions)]
            (Sender::Log, _) => eprintln!("auth-dev: sign-in code for {addr}: {code}"),
            (Sender::Postmark { api, token, from }, Dest::Email(email)) => {
                let res = self
                    .http
                    .post(format!("{api}/email"))
                    .header("X-Postmark-Server-Token", token)
                    .header("Accept", "application/json")
                    .json(&json!({
                        "From": from,
                        "To": email,
                        "Subject": format!("{code} is your GeauxHack sign-in code"),
                        "TextBody": format!(
                            "Your GeauxHack sign-in code is {code}. It expires in 10 minutes.\n\n\
                             If you didn't ask for it, you can ignore this email."
                        ),
                        "MessageStream": "outbound",
                    }))
                    .send()
                    .await
                    .map_err(down)?;
                match res.status().as_u16() {
                    200..=299 => {}
                    429 => return Err(CodeError::TooMany),
                    // 300: address Postmark can't send to; 406: it bounced or complained before.
                    // Other 422s (an unconfirmed sender, say) are our setup, so 503.
                    422 => {
                        let body: Value = res.json().await.unwrap_or_default();
                        let bad = matches!(body["ErrorCode"].as_i64(), Some(300 | 406));
                        return Err(if bad { CodeError::BadAddress } else { CodeError::Unavailable });
                    }
                    _ => return Err(CodeError::Unavailable),
                }
            }
            _ => unreachable!("Codes::new rejects the other pairings"),
        }
        // Stored only once sent, so a failed resend leaves the earlier code working.
        // (If the provider timed out but delivered anyway, that code won't work; the user asks again.)
        let hash = hash(to, &code);
        redis
            .eval::<(), _, _, _>(STORE, code_key(to), vec![hash, (now + CODE_SECS).to_string(), CODE_SECS.to_string()])
            .await
            .map_err(down)
    }

    /// True if `code` is right. A right code works once; each guess uses a try.
    pub async fn check(&self, redis: &Client, to: &Dest, code: &str, ip: &str, now: i64) -> Result<bool, CodeError> {
        limit(redis, CHECK_PER_IP, ip, now).await?;
        if code.len() != 6 || !code.bytes().all(|b| b.is_ascii_digit()) {
            return Ok(false);
        }
        limit(redis, CHECK_PER_DAY, &to.key(), now).await?;
        if let (Sender::TwilioVerify { .. }, Dest::Phone(phone)) = (&self.sms, to) {
            return match self.twilio("VerificationCheck", &[("To", phone), ("Code", code)]).await? {
                (200, body) => Ok(body["status"] == "approved"),
                // No pending code: expired, used, or out of tries.
                (404, _) => Ok(false),
                _ => Err(CodeError::Unavailable),
            };
        }
        let key = code_key(to);
        // Counts the try before comparing, so parallel guesses can't exceed 5.
        let Some(stored) = redis.eval::<Option<Vec<String>>, _, _, _>(TAKE, key.clone(), MAX_TRIES).await.map_err(down)?
        else {
            return Ok(false);
        };
        let [stored_hash, exp] = &stored[..] else { return Ok(false) };
        let right: bool = hash(to, code).as_bytes().ct_eq(stored_hash.as_bytes()).into();
        if !right || now >= exp.parse::<i64>().unwrap_or(0) {
            return Ok(false);
        }
        // Single use, tied to the challenge read above: a resend must not let
        // an old in-flight check consume the replacement or sign in twice.
        consume(redis, &key, stored_hash, exp).await
    }

    async fn twilio(&self, path: &str, form: &[(&str, &str)]) -> Result<(u16, Value), CodeError> {
        let Sender::TwilioVerify { api, account_sid, auth_token, service_sid } = &self.sms else {
            unreachable!("only called with Twilio Verify")
        };
        let res = self
            .http
            .post(format!("{api}/v2/Services/{service_sid}/{path}"))
            .basic_auth(account_sid, Some(auth_token))
            .form(form)
            .send()
            .await
            .map_err(down)?;
        match res.status().as_u16() {
            // Twilio's own limits (too many sends or checks for this number).
            429 => Err(CodeError::TooMany),
            // A 200 we can't read is an outage, not a wrong code: Twilio may already have approved it.
            200 => Ok((200, res.json().await.map_err(down)?)),
            status => Ok((status, Value::Null)),
        }
    }
}

async fn consume(redis: &Client, key: &str, stored_hash: &str, exp: &str) -> Result<bool, CodeError> {
    Ok(redis.eval::<i64, _, _, _>(CONSUME, key, vec![stored_hash, exp]).await.map_err(down)? == 1)
}

/// The account for this email or phone, created on first sign-in (with no roles).
pub async fn user_for(db: &PgPool, to: &Dest) -> Result<String, sqlx::Error> {
    // DO UPDATE (a no-op) rather than DO NOTHING, so RETURNING gives the existing row too.
    let (sql, value) = match to {
        Dest::Email(e) => (
            "INSERT INTO users (email) VALUES ($1)
             ON CONFLICT (email) DO UPDATE SET email = EXCLUDED.email RETURNING id::text",
            e,
        ),
        Dest::Phone(p) => (
            "INSERT INTO users (phone_e164) VALUES ($1)
             ON CONFLICT (phone_e164) DO UPDATE SET phone_e164 = EXCLUDED.phone_e164 RETURNING id::text",
            p,
        ),
    };
    sqlx::query_scalar(sql).bind(value).fetch_one(db).await
}

fn code_key(to: &Dest) -> String {
    format!("gh:code:{}", to.key())
}

/// Hex SHA-256 of address and code, so the same code for two people hashes differently.
// ponytail: unsalted; a million codes are quick to try against a leaked hash, but
// reading Redis already means owning the server. HMAC with a secret if that changes.
fn hash(to: &Dest, code: &str) -> String {
    format!("{:x}", Sha256::digest(format!("{}:{code}", to.key())))
}

/// Six random digits from the OS, every code equally likely.
fn new_code() -> String {
    loop {
        let mut b = [0u8; 4];
        getrandom::getrandom(&mut b).expect("the OS has a random number source");
        let n = u32::from_le_bytes(b);
        // 4_294_000_000 is the largest multiple of a million that fits; above it,
        // low codes would come up slightly more often.
        if n < 4_294_000_000 {
            return format!("{:06}", n % 1_000_000);
        }
    }
}

/// Fixed windows by `now`, so tests can move a fake clock. Redis expires old windows.
async fn limit(redis: &Client, (name, window, max): (&str, i64, i64), who: &str, now: i64) -> Result<(), CodeError> {
    let key = format!("gh:rl:{name}:{window}:{who}:{}", now / window);
    let n: i64 = redis.eval(HIT, key, window).await.map_err(down)?;
    if n > max {
        Err(CodeError::TooMany)
    } else {
        Ok(())
    }
}

// Lua scripts run atomically in Redis, so no key is ever left without its expiry.
const HIT: &str = "
local n = redis.call('INCR', KEYS[1])
if n == 1 then redis.call('EXPIRE', KEYS[1], ARGV[1]) end
return n";

const STORE: &str = "
redis.call('DEL', KEYS[1])
redis.call('HSET', KEYS[1], 'h', ARGV[1], 'exp', ARGV[2], 'tries', 0)
redis.call('EXPIRE', KEYS[1], ARGV[3])";

// Uses up a try. Returns {hash, exp}, or nil if there's no code or no tries left.
// Out of tries it's left for Redis to expire, not deleted: deleting would make a
// right guess that's still being compared lose its single-use DEL.
const TAKE: &str = "
if redis.call('EXISTS', KEYS[1]) == 0 then return false end
if redis.call('HINCRBY', KEYS[1], 'tries', 1) > tonumber(ARGV[1]) then return false end
return redis.call('HMGET', KEYS[1], 'h', 'exp')";

const CONSUME: &str = "
if redis.call('HGET', KEYS[1], 'h') == ARGV[1] and redis.call('HGET', KEYS[1], 'exp') == ARGV[2] then
    return redis.call('DEL', KEYS[1])
end
return 0";

#[cfg(test)]
mod tests {
    use super::*;
    use fred::prelude::{Builder, ClientLike, Config};

    #[tokio::test]
    async fn an_in_flight_check_cannot_consume_a_resent_code() {
        let redis = Builder::from_config(Config::from_url(&std::env::var("REDIS_URL").unwrap()).unwrap())
            .build().unwrap();
        redis.init().await.unwrap();
        let codes = Codes::new(Sender::Log, Sender::Log);
        for already_used in [false, true] {
            let to = Dest::email(&format!("race-{}{}@example.com", new_code(), new_code())).unwrap();
            let key = code_key(&to);
            let old_hash = hash(&to, "111111");
            redis.eval::<(), _, _, _>(STORE, &key, vec![old_hash, "1800000600".into(), CODE_SECS.to_string()]).await.unwrap();
            // Pause a correct check after it has read the old challenge.
            let stored: Vec<String> = redis.eval(TAKE, &key, MAX_TRIES).await.unwrap();
            if already_used {
                assert!(consume(&redis, &key, &stored[0], &stored[1]).await.unwrap());
            }
            // A resend arrives before that in-flight check finishes.
            redis.eval::<(), _, _, _>(STORE, &key, vec![hash(&to, "222222"), "1800000660".into(), CODE_SECS.to_string()]).await.unwrap();
            assert!(!consume(&redis, &key, &stored[0], &stored[1]).await.unwrap(), "old challenge consumed a newer code");
            assert!(codes.check(&redis, &to, "222222", &to.key(), 1_800_000_061).await.unwrap());
        }
    }
}
