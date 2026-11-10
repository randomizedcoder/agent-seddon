//! The listener's mapping from a request to a route, its header screen, and one
//! round trip over a real socket.

use std::sync::Mutex;

use rstest::rstest;

use super::*;

/// A route the fake saw, with what it was handed.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Called {
    Login(String),
    Callback(String, Option<String>, String),
    Logout(String, Option<String>),
    Check(Listener, String, Option<String>, Option<String>),
}

/// Records each call and answers with `reply`.
struct Fake {
    calls: Mutex<Vec<Called>>,
    reply: Reply,
}

impl Fake {
    fn new(reply: Reply) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            reply,
        }
    }

    fn calls(&self) -> Vec<Called> {
        self.calls.lock().expect("calls").clone()
    }

    fn saw(&self, c: Called) -> Reply {
        self.calls.lock().expect("calls").push(c);
        self.reply.clone()
    }
}

#[async_trait]
impl GateRoutes for Fake {
    async fn login(&self, query: &str) -> Reply {
        self.saw(Called::Login(query.into()))
    }
    async fn callback(&self, query: &str, cookie: Option<&str>, user_agent: &str) -> Reply {
        self.saw(Called::Callback(
            query.into(),
            cookie.map(Into::into),
            user_agent.into(),
        ))
    }
    async fn logout(&self, method: &str, cookie: Option<&str>) -> Reply {
        self.saw(Called::Logout(method.into(), cookie.map(Into::into)))
    }
    async fn check(&self, req: CheckRequest<'_>) -> Reply {
        self.saw(Called::Check(
            req.listener,
            req.path.into(),
            req.cookie.map(Into::into),
            req.authorization.map(Into::into),
        ))
    }
}

fn ok() -> Reply {
    Reply::new(200, "")
}

fn s(v: &str) -> String {
    v.to_string()
}

#[rstest]
#[case::positive_login("GET", "/auth/login?rd=%2Fgrafana%2F", &[], Some(Called::Login(s("rd=%2Fgrafana%2F"))), 200)]
#[case::positive_callback_with_cookie_and_agent(
    "GET", "/auth/callback?code=c&state=s",
    &[("Cookie", "__Host-seddon-login=n"), ("User-Agent", "firefox")],
    Some(Called::Callback(s("code=c&state=s"), Some(s("__Host-seddon-login=n")), s("firefox"))), 200
)]
#[case::positive_logout_post("POST", "/auth/logout", &[("cookie", "__Host-seddon=v")], Some(Called::Logout(s("POST"), Some(s("__Host-seddon=v")))), 200)]
#[case::positive_check_main(
    "GET", "/_check/main/grafana/d/x?orgId=1",
    &[("cookie", "a=1"), ("authorization", "Bearer t")],
    Some(Called::Check(Listener::Main, s("/grafana/d/x?orgId=1"), Some(s("a=1")), Some(s("Bearer t")))), 200
)]
#[case::positive_check_hyperdx("GET", "/_check/hyperdx/search", &[], Some(Called::Check(Listener::Hyperdx, s("/search"), None, None)), 200)]
#[case::positive_check_any_method("POST", "/_check/main/agent.v1.X/Y", &[], Some(Called::Check(Listener::Main, s("/agent.v1.X/Y"), None, None)), 200)]
#[case::corner_logout_get_reaches_the_route_which_refuses_it("GET", "/auth/logout", &[], Some(Called::Logout(s("GET"), None)), 200)]
#[case::corner_login_without_query("GET", "/auth/login", &[], Some(Called::Login(s(""))), 200)]
#[case::corner_cookie_headers_joined(
    "GET", "/_check/main/", &[("cookie", "a=1"), ("Cookie", "__Host-seddon=v")],
    Some(Called::Check(Listener::Main, s("/"), Some(s("a=1; __Host-seddon=v")), None)), 200
)]
#[case::corner_header_names_any_case("GET", "/_check/main/", &[("AUTHORIZATION", "Bearer t")], Some(Called::Check(Listener::Main, s("/"), None, Some(s("Bearer t")))), 200)]
#[case::negative_login_post("POST", "/auth/login", &[], None, 405)]
#[case::negative_callback_post("POST", "/auth/callback?code=c&state=s", &[], None, 405)]
#[case::negative_unknown_path("GET", "/", &[], None, 404)]
#[case::negative_auth_prefix_only("GET", "/auth/", &[], None, 404)]
#[case::negative_check_prefix_alone("GET", "/_check/main", &[], None, 404)]
#[case::adversarial_check_prefix_lookalike("GET", "/_check/mainx/grafana/", &[], None, 404)]
#[case::adversarial_login_path_with_suffix("GET", "/auth/login/../callback?code=c", &[], None, 404)]
#[case::adversarial_two_authorization_headers(
    "GET", "/_check/main/agent.v1.X/Y", &[("authorization", "Bearer a"), ("authorization", "Bearer b")], None, 400
)]
#[case::boundary_url_at_the_cap("GET", &format!("/auth/login?rd={}", "a".repeat(MAX_URL - "/auth/login?rd=".len())), &[], Some(Called::Login(format!("rd={}", "a".repeat(MAX_URL - "/auth/login?rd=".len())))), 200)]
#[case::boundary_url_one_over("GET", &format!("/auth/login?rd={}", "a".repeat(MAX_URL - "/auth/login?rd=".len() + 1)), &[], None, 414)]
#[tokio::test]
async fn dispatch_cases(
    #[case] method: &str,
    #[case] url: &str,
    #[case] headers: &[(&str, &str)],
    #[case] want: Option<Called>,
    #[case] status: u16,
) {
    let fake = Fake::new(ok());
    let reply = dispatch(
        &fake,
        Incoming {
            method,
            url,
            headers,
        },
    )
    .await;
    assert_eq!(reply.status, status, "{reply:?}");
    assert_eq!(fake.calls(), want.into_iter().collect::<Vec<_>>());
}

#[rstest]
#[case::positive_login("/auth/login", true)]
#[case::positive_callback("/auth/callback", true)]
#[case::positive_logout("/auth/logout", true)]
#[case::negative_check("/_check/main/", false)]
#[tokio::test]
async fn browser_routes_are_never_cached(#[case] url: &str, #[case] no_store: bool) {
    let fake = Fake::new(ok());
    let method = if url == "/auth/logout" { "POST" } else { "GET" };
    let reply = dispatch(
        &fake,
        Incoming {
            method,
            url,
            headers: &[],
        },
    )
    .await;
    assert_eq!(reply.header("cache-control") == Some("no-store"), no_store);
}

fn with_header(name: &str, value: &str) -> Reply {
    ok().with(name, value.into())
}

#[rstest]
#[case::positive_none(ok(), true)]
#[case::positive_bearer(with_header("authorization", "Bearer eyJ.a-b_c.d"), true)]
#[case::positive_cookie_attributes(
    with_header(
        "set-cookie",
        "__Host-seddon=v; Path=/; Secure; HttpOnly; SameSite=Lax"
    ),
    true
)]
#[case::boundary_tilde(with_header("location", "/~a"), true)]
#[case::negative_empty_name(with_header("", "v"), false)]
#[case::negative_space_in_name(with_header("x seddon", "v"), false)]
#[case::boundary_del(with_header("x", "a\u{7f}"), false)]
#[case::adversarial_crlf_value(with_header("x-seddon-user", "u\r\nx-seddon-role: Admin"), false)]
#[case::adversarial_bare_lf(with_header("location", "/\nset-cookie: x"), false)]
#[case::adversarial_nul(with_header("location", "/\0"), false)]
#[case::adversarial_tab(with_header("location", "/\t"), false)]
#[case::adversarial_non_ascii(with_header("x-seddon-user", "ü"), false)]
#[case::adversarial_colon_in_name(with_header("x:y", "v"), false)]
fn writable_cases(#[case] reply: Reply, #[case] want: bool) {
    assert_eq!(writable(&reply), want, "{reply:?}");
}

/// Serve `fake` on a loopback port.
fn serving(fake: Fake) -> String {
    let server = tiny_http::Server::http("127.0.0.1:0").expect("bind");
    let addr = server.server_addr().to_ip().expect("ip");
    serve(server, Arc::new(fake), tokio::runtime::Handle::current()).expect("serve");
    format!("http://{addr}")
}

fn no_redirects() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("client")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn positive_round_trip_writes_status_and_every_header() {
    let reply = Reply::new(302, "")
        .with("location", "/grafana/".into())
        .with("set-cookie", "a=; Max-Age=0".into())
        .with("set-cookie", "b=v".into());
    let base = serving(Fake::new(reply));
    let resp = no_redirects()
        .get(format!("{base}/auth/callback?code=c&state=s"))
        .header("cookie", "__Host-seddon-login=n")
        .send()
        .await
        .expect("answered");
    assert_eq!(resp.status().as_u16(), 302);
    let h = resp.headers();
    assert_eq!(
        h.get("location").and_then(|v| v.to_str().ok()),
        Some("/grafana/")
    );
    let cookies: Vec<&str> = h
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .collect();
    assert_eq!(cookies, ["a=; Max-Age=0", "b=v"]);
    assert_eq!(
        h.get("cache-control").and_then(|v| v.to_str().ok()),
        Some("no-store")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn adversarial_unwritable_answer_becomes_500_with_none_of_its_headers() {
    let reply = ok().with("x-seddon-user", "u\r\nx-seddon-role: Admin".into());
    let base = serving(Fake::new(reply));
    let resp = no_redirects()
        .get(format!("{base}/_check/main/grafana/"))
        .send()
        .await
        .expect("answered");
    assert_eq!(resp.status().as_u16(), 500);
    assert!(resp.headers().get("x-seddon-user").is_none());
    assert!(resp.headers().get("x-seddon-role").is_none());
}
