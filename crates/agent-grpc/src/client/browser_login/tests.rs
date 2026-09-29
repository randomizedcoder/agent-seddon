//! The loopback callback: which requests end the sign-in, and the listener's
//! behaviour against a real socket.

use std::time::Duration;

use rstest::rstest;

use super::*;

const STATE: &str = "Zm9vYmFyYmF6cXV4cXV1eHh5enp5eHd2dXRzcnFwb25t";

fn cb(query: &str) -> String {
    format!("{CALLBACK_PATH}?{query}")
}

#[rstest]
#[case::positive_code(cb(&format!("code=abc&state={STATE}")), CallbackAnswer::Code("abc".into()))]
#[case::positive_extra_params_ignored(
    cb(&format!("state={STATE}&code=abc&scope=openid&authuser=0")),
    CallbackAnswer::Code("abc".into())
)]
#[case::positive_denied(
    cb(&format!("error=access_denied&state={STATE}")),
    CallbackAnswer::Denied("access_denied".into())
)]
#[case::negative_wrong_state(cb("code=abc&state=other"), CallbackAnswer::Ignore(400))]
#[case::negative_no_state(cb("code=abc"), CallbackAnswer::Ignore(400))]
#[case::negative_no_code(cb(&format!("state={STATE}")), CallbackAnswer::Ignore(400))]
#[case::negative_empty_code(cb(&format!("code=&state={STATE}")), CallbackAnswer::Ignore(400))]
#[case::negative_other_path(format!("/favicon.ico?state={STATE}&code=abc"), CallbackAnswer::Ignore(404))]
#[case::corner_path_prefix(
    format!("{CALLBACK_PATH}x?state={STATE}&code=abc"),
    CallbackAnswer::Ignore(404)
)]
#[case::corner_error_without_state_does_not_end_it(
    cb("error=access_denied"),
    CallbackAnswer::Ignore(400)
)]
#[case::boundary_code_at_cap(
    cb(&format!("code={}&state={STATE}", "c".repeat(MAX_CODE_BYTES))),
    CallbackAnswer::Code("c".repeat(MAX_CODE_BYTES))
)]
#[case::boundary_code_over_cap(
    cb(&format!("code={}&state={STATE}", "c".repeat(MAX_CODE_BYTES + 1))),
    CallbackAnswer::Ignore(400)
)]
#[case::adversarial_oversized_url(
    cb(&format!("code=abc&state={STATE}&pad={}", "p".repeat(MAX_CALLBACK_URL_BYTES))),
    CallbackAnswer::Ignore(414)
)]
#[case::adversarial_two_states(
    cb(&format!("code=abc&state=other&state={STATE}")),
    CallbackAnswer::Ignore(400)
)]
#[case::adversarial_two_codes(
    cb(&format!("code=abc&code=evil&state={STATE}")),
    CallbackAnswer::Ignore(400)
)]
#[case::adversarial_control_chars_in_error(
    cb(&format!("error=%1b%5b2J&state={STATE}")),
    CallbackAnswer::Denied("unspecified".into())
)]
#[case::adversarial_absolute_form(
    format!("http://evil.example{CALLBACK_PATH}?code=abc&state={STATE}"),
    CallbackAnswer::Ignore(404)
)]
#[case::adversarial_dot_segments(
    format!("/x/..{CALLBACK_PATH}?code=abc&state={STATE}"),
    CallbackAnswer::Code("abc".into())
)]
fn classify_callback_cases(#[case] url: String, #[case] want: CallbackAnswer) {
    assert_eq!(classify_callback(&url, STATE), want, "{url}");
}

#[test]
fn positive_verifier_is_a_valid_pkce_verifier_and_fresh() {
    let a = new_verifier().unwrap();
    let b = new_verifier().unwrap();
    assert!(crate::server::is_verifier(&a), "{a}");
    assert_eq!(a.len(), 43);
    assert_ne!(a, b);
}

/// GET `url` on the listener at `port`; the status, or `None` when nothing answers.
async fn get(port: u16, url: &str) -> Option<u16> {
    let resp = reqwest::Client::new()
        .get(format!("http://127.0.0.1:{port}{url}"))
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .ok()?;
    Some(resp.status().as_u16())
}

#[tokio::test]
async fn positive_listener_returns_the_code_after_ignoring_strays() {
    let callback = Callback::bind().unwrap();
    let uri = callback.redirect_uri();
    let port: u16 = uri
        .trim_start_matches("http://127.0.0.1:")
        .trim_end_matches(CALLBACK_PATH)
        .parse()
        .unwrap();
    assert_eq!(uri, format!("http://127.0.0.1:{port}{CALLBACK_PATH}"));
    let waiting = tokio::spawn(callback.wait(STATE.into(), Duration::from_secs(10)));

    // Strays first: none of them ends the sign-in.
    assert_eq!(get(port, "/favicon.ico").await, Some(404));
    assert_eq!(get(port, &cb("code=evil&state=guess")).await, Some(400));
    assert_eq!(get(port, &cb("error=access_denied")).await, Some(400));
    assert_eq!(
        get(port, &cb(&format!("code=abc&state={STATE}"))).await,
        Some(200)
    );
    assert_eq!(waiting.await.unwrap(), Ok("abc".into()));

    // The listener is gone: a second callback finds nobody.
    assert_eq!(
        get(port, &cb(&format!("code=again&state={STATE}"))).await,
        None
    );
}

#[tokio::test]
async fn negative_listener_reports_the_idp_refusal() {
    let callback = Callback::bind().unwrap();
    let port = callback.port;
    let waiting = tokio::spawn(callback.wait(STATE.into(), Duration::from_secs(10)));
    get(port, &cb(&format!("error=access_denied&state={STATE}"))).await;
    let e = waiting.await.unwrap().unwrap_err();
    assert!(e.contains("access_denied"), "{e}");
}

#[tokio::test]
async fn boundary_listener_times_out() {
    let callback = Callback::bind().unwrap();
    let e = callback
        .wait(STATE.into(), Duration::from_millis(300))
        .await
        .unwrap_err();
    assert!(e.contains("within"), "{e}");
}

#[tokio::test]
async fn adversarial_post_to_the_callback_is_refused() {
    let callback = Callback::bind().unwrap();
    let port = callback.port;
    let waiting = tokio::spawn(callback.wait(STATE.into(), Duration::from_secs(10)));
    let status = reqwest::Client::new()
        .post(format!(
            "http://127.0.0.1:{port}{}",
            cb(&format!("code=abc&state={STATE}"))
        ))
        .send()
        .await
        .unwrap()
        .status();
    assert_eq!(status.as_u16(), 405);
    get(port, &cb(&format!("code=real&state={STATE}"))).await;
    assert_eq!(waiting.await.unwrap(), Ok("real".into()));
}
