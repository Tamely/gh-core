use super::*;
use axum::{body::Body, http::Request, middleware::from_fn_with_state, routing::get};
use ed25519_dalek::SigningKey;
use fred::prelude::{Builder, ClientLike, Config as RedisConfig};
use sqlx::{postgres::PgConnectOptions, PgPool};
use tower::ServiceExt;

const NOW: i64 = 1_800_000_000;
const ORIGIN: &str = "https://app.geauxhack.org";
const VERIFIER: &str = "abcdefghijklmnopqrstuvwxyz0123456789ABCDEFG";

async fn setup(schema: &str) -> Arc<AuthState> {
    let url = std::env::var("DATABASE_URL").expect("set DATABASE_URL");
    let admin = PgPool::connect(&url).await.unwrap();
    sqlx::raw_sql(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE; CREATE SCHEMA {schema}"))
        .execute(&admin)
        .await
        .unwrap();
    let opts: PgConnectOptions = url.parse().unwrap();
    let db = PgPool::connect_with(opts.options([("search_path", schema)])).await.unwrap();
    sqlx::raw_sql(include_str!("../../../migrations/0001_users_and_auth.sql")).execute(&db).await.unwrap();
    let redis = Builder::from_config(RedisConfig::from_url(&std::env::var("REDIS_URL").unwrap()).unwrap())
        .with_performance_config(|c| c.default_command_timeout = Duration::from_secs(1))
        .build()
        .unwrap();
    redis.init().await.unwrap();
    // Other tests use a separate IP for rate-limit assertions.
    let _: i64 = redis.del(format!("gh:oauth:rl:192.0.2.10:{}", NOW / 3600)).await.unwrap();
    Arc::new(AuthState {
        keys: crate::Keys::new(SigningKey::from_bytes(&[1; 32]), vec![]),
        origin: ORIGIN.into(),
        now: || NOW,
        sessions: crate::Sessions::new(db, redis),
        policies: crate::policies(),
        codes: crate::Codes::new(crate::Sender::Log, crate::Sender::Log),
        oauth: OAuth::from_env().unwrap(),
    })
}

async fn mock(app: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

fn configure(state: &mut Arc<AuthState>, provider: Provider, base: &str) {
    Arc::get_mut(state).unwrap().oauth.providers.insert(
        provider,
        Config {
            client_id: "client".into(),
            secret: Secret::Shared("secret".into()),
            authorize: format!("{base}/authorize"),
            token: format!("{base}/token"),
            identity: format!("{base}/identity"),
        },
    );
}

async fn call(state: &Arc<AuthState>, path: &str, body: Value, origin: Option<&str>) -> (u16, HeaderMap, Value) {
    let app = crate::routes(state.clone()).layer(from_fn_with_state(state.clone(), crate::layer));
    let mut request = Request::post(path)
        .header(header::CONTENT_TYPE, "application/json")
        .extension(ConnectInfo("192.0.2.10:1234".parse::<SocketAddr>().unwrap()));
    if let Some(origin) = origin {
        request = request.header(header::ORIGIN, origin);
    }
    let response = app.oneshot(request.body(Body::from(body.to_string())).unwrap()).await.unwrap();
    let status = response.status().as_u16();
    let headers = response.headers().clone();
    assert_eq!(headers[header::CACHE_CONTROL], "no-store");
    if path.starts_with("/api/v1/auth/oauth/") {
        assert_eq!(headers[header::REFERRER_POLICY], "no-referrer");
    }
    let body = axum::body::to_bytes(response.into_body(), 1 << 16).await.unwrap();
    (status, headers, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

async fn begin(state: &Arc<AuthState>, provider: &str, origin: Option<&str>) -> Value {
    let (status, _, result) = call(
        state,
        &format!("/api/v1/auth/oauth/{provider}/start"),
        json!({"codeChallenge": B64.encode(Sha256::digest(VERIFIER.as_bytes()))}),
        origin,
    )
    .await;
    assert_eq!(status, 200);
    result
}

fn completion(begin: &Value) -> Value {
    json!({"state": begin["state"], "code": "good-code", "codeVerifier": VERIFIER})
}

#[tokio::test]
async fn code_then_github_share_account_roles_and_web_and_native_sessions() {
    let email = format!("{}@example.com", random().unwrap().to_ascii_lowercase());
    let verified_email = email.clone();
    let delivered = Arc::new(Mutex::new(String::new()));
    let captured = delivered.clone();
    let base = mock(
        Router::new()
            .route(
                "/token",
                post(|Form(form): Form<HashMap<String, String>>| async move {
                    assert_eq!(form["code_verifier"], VERIFIER);
                    assert_eq!(form["redirect_uri"], format!("{ORIGIN}/auth/callback"));
                    assert_eq!(form["client_secret"], "secret");
                    Json(json!({"access_token": "provider-secret"}))
                }),
            )
            .route(
                "/email",
                post(move |Json(body): Json<Value>| {
                    let captured = captured.clone();
                    async move {
                        *captured.lock().await =
                            body["Subject"].as_str().unwrap().split_whitespace().next().unwrap().into();
                        StatusCode::OK
                    }
                }),
            )
            .route(
                "/identity",
                get(move |headers: HeaderMap| {
                    let email = verified_email.clone();
                    async move {
                        assert_eq!(headers[header::AUTHORIZATION], "Bearer provider-secret");
                        Json(json!([{"email": "unverified@example.com", "primary": false, "verified": false},
            {"email": email.to_ascii_uppercase(), "primary": true, "verified": true}]))
                    }
                }),
            ),
    )
    .await;
    let mut state = setup("oauth_github").await;
    configure(&mut state, Provider::Github, &base);
    Arc::get_mut(&mut state).unwrap().codes = crate::Codes::new(
        crate::Sender::Postmark { api: base, token: "test".into(), from: "auth@example.com".into() },
        crate::Sender::Log,
    );
    let to = Dest::email(&email).unwrap();
    state.codes.send(&state.sessions.redis, &to, &random().unwrap(), NOW).await.unwrap();
    let code = delivered.lock().await.clone();
    let (status, headers, signed_in) =
        call(&state, "/api/v1/auth/code/verify", json!({"email": email, "code": code}), Some(ORIGIN)).await;
    assert_eq!(status, 200);
    assert!(headers.contains_key(header::SET_COOKIE));
    assert_eq!(signed_in["roles"], json!([]));
    let id = signed_in["id"].as_str().unwrap().to_owned();
    sqlx::query("INSERT INTO user_roles (user_id, role) VALUES ($1::uuid, 'MENTOR')")
        .bind(&id)
        .execute(&state.sessions.db)
        .await
        .unwrap();
    for origin in [Some(ORIGIN), None] {
        let begin = begin(&state, "github", origin).await;
        let url = Url::parse(begin["url"].as_str().unwrap()).unwrap();
        let params: HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(params["code_challenge_method"], "S256");
        assert_eq!(params["scope"], "user:email");
        let (status, headers, body) =
            call(&state, "/api/v1/auth/oauth/github/complete", completion(&begin), origin).await;
        assert_eq!(status, 200);
        assert_eq!(body["id"], id);
        assert_eq!(body["roles"], json!(["MENTOR"]));
        let token = if origin.is_some() {
            assert!(body.get("token").is_none());
            let cookie = headers[header::SET_COOKIE].to_str().unwrap();
            assert!(cookie.contains("Secure; HttpOnly; SameSite=Lax"));
            assert!(cookie.contains("Path=/; Max-Age=1209600"));
            cookie.split(';').next().unwrap().split_once('=').unwrap().1
        } else {
            assert!(!headers.contains_key(header::SET_COOKIE));
            body["token"].as_str().unwrap()
        };
        let claims = state.keys.verify(token, NOW).unwrap();
        assert_eq!(claims.sub, id);
        assert_eq!(claims.exp, NOW + crate::SESSION_SECS);
        assert_eq!(call(&state, "/api/v1/auth/oauth/github/complete", completion(&begin), origin).await.0, 401);
        let (auth_header, auth_value) = if origin.is_some() {
            (header::COOKIE, format!("{}={token}", crate::COOKIE))
        } else {
            (header::AUTHORIZATION, format!("Bearer {token}"))
        };
        let app = crate::routes(state.clone()).layer(from_fn_with_state(state.clone(), crate::layer));
        if origin.is_some() {
            let request = Request::post("/api/v1/auth/logout")
                .header(auth_header.clone(), &auth_value)
                .header(header::ORIGIN, "https://evil.example")
                .body(Body::empty())
                .unwrap();
            assert_eq!(app.clone().oneshot(request).await.unwrap().status(), 403);
        }
        // Reuse the original credential even after logout: rejection must come
        // from server-side revocation, not just the browser deleting its cookie.
        for (method, path, status) in
            [("GET", "/api/v1/me", 200), ("POST", "/api/v1/auth/logout", 204), ("GET", "/api/v1/me", 401)]
        {
            let request = Request::builder()
                .method(method)
                .uri(path)
                .header(auth_header.clone(), &auth_value)
                .header(header::ORIGIN, ORIGIN)
                .body(Body::empty())
                .unwrap();
            let response = app.clone().oneshot(request).await.unwrap();
            assert_eq!(response.status(), status, "{method} {path}");
            if status == 204 {
                assert!(response.headers()[header::SET_COOKIE].to_str().unwrap().contains("Max-Age=0"));
            }
        }
    }
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM users").fetch_one(&state.sessions.db).await.unwrap(),
        1
    );
}

#[tokio::test]
async fn state_is_bound_to_provider_verifier_origin_and_single_use() {
    let base = mock(Router::new().route("/token", post(|| async { Json(json!({"access_token": "t"})) })).route(
        "/identity",
        get(|| async { Json(json!([{"email": "new@example.com", "primary": true, "verified": true}])) }),
    ))
    .await;
    let mut state = setup("oauth_binding").await;
    configure(&mut state, Provider::Github, &base);
    configure(&mut state, Provider::Google, &base);
    let started = begin(&state, "github", Some(ORIGIN)).await;
    let body = completion(&started);
    let path = "/api/v1/auth/oauth/github/complete";
    assert_eq!(call(&state, path, body.clone(), None).await.0, 401);
    assert_eq!(call(&state, path, body.clone(), Some("https://evil.example")).await.0, 401);
    assert_eq!(call(&state, "/api/v1/auth/oauth/google/complete", body.clone(), Some(ORIGIN)).await.0, 401);
    let mut wrong = body.clone();
    wrong["codeVerifier"] = "wrong-verifier-abcdefghijklmnopqrstuvwxyz0123456789".into();
    assert_eq!(call(&state, path, wrong, Some(ORIGIN)).await.0, 401);
    let (a, b) = tokio::join!(call(&state, path, body.clone(), Some(ORIGIN)), call(&state, path, body, Some(ORIGIN)));
    let mut statuses = [a.0, b.0];
    statuses.sort();
    assert_eq!(statuses, [200, 401]);
    // The new account has no implicit privilege.
    let roles: i64 = sqlx::query_scalar("SELECT count(*) FROM user_roles").fetch_one(&state.sessions.db).await.unwrap();
    assert_eq!(roles, 0);
    let started = begin(&state, "github", None).await;
    let key = format!("gh:oauth:{}", started["state"].as_str().unwrap());
    let ttl: i64 = state.sessions.redis.ttl(&key).await.unwrap();
    assert!((1..=TTL).contains(&ttl));
    let _: i64 = state.sessions.redis.del(key).await.unwrap();
    assert_eq!(call(&state, path, completion(&started), None).await.0, 401);
}

#[tokio::test]
async fn rejects_unverified_email_and_provider_failures_without_creating_users() {
    let mut state = setup("oauth_failures").await;
    for (token_status, identity_status, emails, expected) in [
        (200, 200, json!([{"email": "victim@example.com", "verified": false, "primary": true}]), 401),
        (200, 200, json!([{"email": "x<victim@example.com>", "verified": true, "primary": true}]), 401),
        (200, 200, json!([]), 401),
        (400, 200, json!([]), 401),
        (429, 200, json!([]), 429),
        (500, 200, json!([]), 503),
        (200, 500, json!([]), 503),
    ] {
        let base = mock(
            Router::new()
                .route(
                    "/token",
                    post(move || async move {
                        (StatusCode::from_u16(token_status).unwrap(), Json(json!({"access_token": "t"})))
                    }),
                )
                .route(
                    "/identity",
                    get(move || {
                        let emails = emails.clone();
                        async move { (StatusCode::from_u16(identity_status).unwrap(), Json(emails)) }
                    }),
                ),
        )
        .await;
        configure(&mut state, Provider::Github, &base);
        let started = begin(&state, "github", None).await;
        assert_eq!(call(&state, "/api/v1/auth/oauth/github/complete", completion(&started), None).await.0, expected);
    }
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM users").fetch_one(&state.sessions.db).await.unwrap(),
        0
    );
}

#[tokio::test]
async fn starts_are_rate_limited_and_fail_closed_without_a_client_ip_or_redis() {
    let mut state = setup("oauth_limits").await;
    configure(&mut state, Provider::Github, "http://127.0.0.1:1");
    let key = format!("gh:oauth:rl:192.0.2.200:{}", NOW / 3600);
    state
        .sessions
        .redis
        .set::<(), _, _>(&key, 300, Some(fred::types::Expiration::EX(3600)), None, false)
        .await
        .unwrap();
    let app = crate::routes(state.clone()).layer(from_fn_with_state(state.clone(), crate::layer));
    let body = json!({"codeChallenge": B64.encode(Sha256::digest(VERIFIER.as_bytes()))}).to_string();
    // The first header entry is attacker-controlled; only the proxy's last entry counts.
    let response = app
        .clone()
        .oneshot(
            Request::post("/api/v1/auth/oauth/github/start")
                .header(header::CONTENT_TYPE, "application/json")
                .header("x-forwarded-for", "198.51.100.1, 192.0.2.200")
                .body(Body::from(body.clone()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 429);
    let ttl: i64 = state.sessions.redis.ttl(key).await.unwrap();
    assert!((1..=3600).contains(&ttl));
    let response = app
        .oneshot(
            Request::post("/api/v1/auth/oauth/github/start")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 500);
    // Shut down a dedicated Redis client, leaving the shared server alone.
    let _: () = state.sessions.redis.quit().await.unwrap();
    assert_eq!(
        call(
            &state,
            "/api/v1/auth/oauth/github/start",
            json!({"codeChallenge": B64.encode(Sha256::digest(VERIFIER.as_bytes()))}),
            None
        )
        .await
        .0,
        503
    );
}

#[tokio::test]
async fn provider_configuration_errors_are_retryable_and_redirects_are_not_followed() {
    for (status, body, expected) in [
        (200, json!({"error": "bad_verification_code"}), 401),
        (400, json!({"error": "invalid_grant"}), 401),
        (400, json!({"error": "invalid_client"}), 503),
        (200, json!({"error": "incorrect_client_credentials"}), 503),
    ] {
        let base = mock(Router::new().route(
            "/token",
            post(move || {
                let body = body.clone();
                async move { (StatusCode::from_u16(status).unwrap(), Json(body)) }
            }),
        ))
        .await;
        let client = OAuth::from_env().unwrap().client;
        assert_eq!(
            provider_json::<Value>(client.post(format!("{base}/token")).send().await.unwrap())
                .await
                .unwrap_err()
                .as_u16(),
            expected
        );
    }
    let base = mock(Router::new().route(
        "/token",
        post(|| async { (StatusCode::TEMPORARY_REDIRECT, [(header::LOCATION, "http://127.0.0.1:1/steal-secret")]) }),
    ))
    .await;
    let response = OAuth::from_env().unwrap().client.post(format!("{base}/token")).send().await.unwrap();
    assert_eq!(response.status(), 307);
    assert_eq!(provider_json::<Value>(response).await.unwrap_err(), DOWN);
}

#[tokio::test]
async fn oidc_validates_signature_issuer_audience_nonce_expiry_and_verified_email() {
    // Test RSA key generated for this fixture only.
    let key = jsonwebtoken::EncodingKey::from_rsa_pem(include_bytes!("test-key.pem")).unwrap();
    let jwks: Value = serde_json::from_str(include_str!("test-jwks.json")).unwrap();
    let mut state = setup("oauth_oidc").await;
    let mut linked_id = None;
    for provider in [Provider::Google, Provider::Apple] {
        let name = if provider == Provider::Google { "google" } else { "apple" };
        let issuer =
            if provider == Provider::Google { "https://accounts.google.com" } else { "https://appleid.apple.com" };
        let fetches = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        for defect in [
            "none",
            "signature",
            "algorithm",
            "kid",
            "issuer",
            "audience",
            "nonce",
            "expired",
            "future",
            "unverified",
            "missing-email",
            "azp",
            "multiple-audiences",
        ] {
            let tokens = Arc::new(Mutex::new(Value::Null));
            let response = tokens.clone();
            let jwks = jwks.clone();
            let fetches = fetches.clone();
            let base = mock(
                Router::new()
                    .route(
                        "/token",
                        post(move |Form(form): Form<HashMap<String, String>>| {
                            let response = response.clone();
                            async move {
                                if provider == Provider::Apple {
                                    assert!(!form.contains_key("code_verifier"));
                                    let secret = &form["client_secret"];
                                    let header = decode_header(secret).unwrap();
                                    assert_eq!(header.alg, Algorithm::ES256);
                                    assert_eq!(header.kid.as_deref(), Some("apple-test"));
                                    let mut validation = Validation::new(Algorithm::ES256);
                                    validation.validate_exp = false;
                                    validation.set_audience(&["https://appleid.apple.com"]);
                                    validation.set_issuer(&["team"]);
                                    let claims = decode::<Value>(
                                        secret,
                                        &DecodingKey::from_ec_pem(include_bytes!("test-apple-public.pem")).unwrap(),
                                        &validation,
                                    )
                                    .unwrap()
                                    .claims;
                                    assert_eq!(claims["sub"], "client");
                                    assert_eq!(claims["iat"], NOW);
                                    assert_eq!(claims["exp"], NOW + 300);
                                } else {
                                    assert_eq!(form["code_verifier"], VERIFIER);
                                }
                                Json(response.lock().await.clone())
                            }
                        }),
                    )
                    .route(
                        "/identity",
                        get(move || {
                            let jwks = jwks.clone();
                            let fetches = fetches.clone();
                            async move {
                                fetches.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                                Json(jwks)
                            }
                        }),
                    ),
            )
            .await;
            configure(&mut state, provider, &base);
            if provider == Provider::Apple {
                Arc::get_mut(&mut state).unwrap().oauth.providers.get_mut(&provider).unwrap().secret = Secret::Apple {
                    team: "team".into(),
                    kid: "apple-test".into(),
                    key: jsonwebtoken::EncodingKey::from_ec_pem(include_bytes!("test-apple-key.pem")).unwrap(),
                };
            }
            let started = begin(&state, name, None).await;
            let url = Url::parse(started["url"].as_str().unwrap()).unwrap();
            let params: HashMap<_, _> = url.query_pairs().into_owned().collect();
            if provider == Provider::Apple {
                assert_eq!(params["response_mode"], "form_post");
                assert!(!params.contains_key("code_challenge"));
            }
            let mut claims = json!({"iss": issuer, "aud": "client", "sub": "provider-user", "exp": NOW + 300,
                "iat": NOW, "email": "oidc@example.com", "email_verified": true, "nonce": params["nonce"]});
            if provider == Provider::Apple {
                claims["email_verified"] = "true".into();
            }
            match defect {
                "issuer" => claims["iss"] = "https://evil.example".into(),
                "audience" => claims["aud"] = "other-client".into(),
                "nonce" => claims["nonce"] = "wrong".into(),
                "expired" => claims["exp"] = NOW.into(),
                "future" => claims["iat"] = (NOW + 61).into(),
                "unverified" => claims["email_verified"] = false.into(),
                "missing-email" => {
                    claims.as_object_mut().unwrap().remove("email");
                }
                "azp" => claims["azp"] = "other-client".into(),
                "multiple-audiences" => claims["aud"] = json!(["client", "other-client"]),
                _ => {}
            }
            let mut header = jsonwebtoken::Header::new(Algorithm::RS256);
            header.kid = Some(if defect == "kid" { "unknown" } else { "test" }.into());
            let token = if defect == "algorithm" {
                header.alg = Algorithm::HS256;
                jsonwebtoken::encode(&header, &claims, &jsonwebtoken::EncodingKey::from_secret(b"secret")).unwrap()
            } else {
                let mut token = jsonwebtoken::encode(&header, &claims, &key).unwrap();
                if defect == "signature" {
                    let i = token.rfind('.').unwrap() + 1;
                    token.replace_range(i..i + 1, "!");
                }
                token
            };
            *tokens.lock().await = json!({"id_token": token});
            let (status, _, profile) =
                call(&state, &format!("/api/v1/auth/oauth/{name}/complete"), completion(&started), None).await;
            assert_eq!(status, if defect == "none" { 200 } else { 401 }, "{name}: {defect}");
            if defect == "none" {
                let id = profile["id"].as_str().unwrap().to_owned();
                assert_eq!(linked_id.get_or_insert(id.clone()), &id, "verified email links Google and Apple");
                assert_eq!(profile["roles"], json!([]));
            }
        }
        assert_eq!(fetches.load(std::sync::atomic::Ordering::SeqCst), 1, "JWKS must be cached across sign-ins");
    }
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM users").fetch_one(&state.sessions.db).await.unwrap(),
        1
    );
}

#[tokio::test]
async fn apple_post_relays_only_code_state_and_a_fixed_error() {
    let state = setup("oauth_relay").await;
    let app = crate::routes(state.clone()).layer(from_fn_with_state(state.clone(), crate::layer));
    let id = random().unwrap();
    let token = state.keys.issue(&crate::Claims {
        sub: "user".into(),
        sid: "sid".into(),
        roles: vec![],
        room_id: None,
        iat: NOW,
        exp: NOW + 300,
    });
    for (body, expected) in [
        (format!("state={id}&code=a%26b&user=untrusted&id_token=untrusted"), "code=a%26b"),
        (format!("state={id}&error=arbitrary%0d%0a"), "error=access_denied"),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::post("/auth/callback")
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .header(header::COOKIE, format!("{}={token}", crate::COOKIE))
                    .header(header::ORIGIN, "https://appleid.apple.com")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 303);
        let location = response.headers()[header::LOCATION].to_str().unwrap();
        assert!(location.starts_with(&format!("{ORIGIN}/auth/callback?")));
        assert!(location.contains(expected));
        assert!(!location.contains("untrusted"));
        assert!(!response.headers().contains_key(header::SET_COOKIE));
    }
}
