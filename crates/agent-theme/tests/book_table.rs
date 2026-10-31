//! The `ThemeService` tables from docs/design/visual/07-testing.md, run against
//! [`ThemeBook`] over an in-memory store. Each row is a few setup steps (which
//! must succeed), one action, and the outcome that action must have, so each row
//! proves one rule about who may see or change what.

use std::sync::Arc;

use agent_config_store::MemoryBackend;
use agent_proto::pb;
use agent_theme::book::{prefs_id, MAX_THEMES_PER_TENANT};
use agent_theme::{builtins, BookError, Caller, ThemeBook, ThemeErrorKind, FALLBACK_THEME};
use rstest::rstest;

/// The people in the tables. Alice, Bob and Admin share the `acme` tenant; only
/// Admin holds `(approve, theme)`. `AliceElsewhere` has Alice's subject in another
/// tenant, so a row can show the tenant, not the subject, is the boundary.
#[derive(Clone, Copy, Debug)]
enum Who {
    Alice,
    Bob,
    Admin,
    AliceElsewhere,
}

fn caller(who: Who) -> Caller {
    let (tenant, subject, may_publish) = match who {
        Who::Alice => ("acme", "alice", false),
        Who::Bob => ("acme", "bob", false),
        Who::Admin => ("acme", "admin", true),
        Who::AliceElsewhere => ("globex", "alice", false),
    };
    Caller {
        tenant: tenant.into(),
        subject: subject.into(),
        may_publish,
    }
}

/// One call on the book.
#[derive(Clone, Copy)]
enum Step {
    /// Put a user theme `id` (a renamed `windows`), changed by the fn.
    Put(Who, &'static str, fn(&mut pb::Theme)),
    Get(Who, &'static str),
    Delete(Who, &'static str),
    SetActive(Who, &'static str, pb::ThemeMode),
    /// `SetActive` with a raw mode number, for out-of-range modes.
    SetActiveRaw(Who, &'static str, i32),
    /// Put `n` private themes `fill-000`… for `who`.
    Fill(Who, usize),
}

/// What an action must come to.
#[derive(Debug, PartialEq)]
enum Expect {
    Ok,
    NotFound,
    Denied,
    Conflict,
    Invalid(ThemeErrorKind),
    Full,
}

fn outcome<T>(r: Result<T, BookError>) -> Expect {
    match r {
        Ok(_) => Expect::Ok,
        Err(BookError::NotFound(_)) => Expect::NotFound,
        Err(BookError::Denied(_)) => Expect::Denied,
        Err(BookError::Conflict(_)) => Expect::Conflict,
        Err(BookError::Invalid(e)) => Expect::Invalid(e.kind()),
        Err(BookError::Full) => Expect::Full,
        Err(BookError::Store(e)) => panic!("the memory store failed: {e}"),
    }
}

fn user_theme(id: &str) -> pb::Theme {
    let mut t = builtins()
        .unwrap()
        .into_iter()
        .find(|t| t.id == "windows")
        .unwrap();
    t.id = id.into();
    t.name = format!("Theme {id}");
    t.builtin = false;
    t
}

async fn run(book: &ThemeBook, step: Step) -> Expect {
    match step {
        Step::Put(who, id, change) => {
            let mut t = user_theme(id);
            change(&mut t);
            outcome(book.put(&caller(who), t).await)
        }
        Step::Get(who, id) => outcome(book.get(&caller(who), id).await),
        Step::Delete(who, id) => outcome(book.delete(&caller(who), id).await),
        Step::SetActive(who, id, mode) => {
            outcome(book.set_active(&caller(who), id, mode as i32).await)
        }
        Step::SetActiveRaw(who, id, mode) => outcome(book.set_active(&caller(who), id, mode).await),
        Step::Fill(who, n) => {
            for i in 0..n {
                let got = outcome(
                    book.put(&caller(who), user_theme(&format!("fill-{i:03}")))
                        .await,
                );
                if got != Expect::Ok {
                    return got;
                }
            }
            Expect::Ok
        }
    }
}

fn book(default_theme: &str) -> ThemeBook {
    ThemeBook::new(Arc::new(MemoryBackend::new()), default_theme).unwrap()
}

async fn setup(book: &ThemeBook, steps: &[Step]) {
    for (i, step) in steps.iter().enumerate() {
        assert_eq!(run(book, *step).await, Expect::Ok, "setup step {i} failed");
    }
}

// Theme changes for `Step::Put`.
fn same(_: &mut pb::Theme) {}
fn v1(t: &mut pb::Theme) {
    t.version = 1;
}
fn v5(t: &mut pb::Theme) {
    t.version = 5;
}
fn shared(t: &mut pb::Theme) {
    t.visibility = pb::Visibility::Tenant as i32;
}
fn shared_v1(t: &mut pb::Theme) {
    shared(t);
    v1(t);
}
fn radius_33(t: &mut pb::Theme) {
    t.shape.get_or_insert_with(Default::default).corner_radius = 33;
}
fn visibility_99(t: &mut pb::Theme) {
    t.visibility = 99;
}
fn hostile_fields(t: &mut pb::Theme) {
    t.builtin = true;
    t.owner = "bob".into();
}

struct Case {
    description: &'static str,
    setup: &'static [Step],
    action: Step,
    expected: Expect,
}

use Step::*;
use Who::*;

#[rstest]
// positive: the round trip each role is meant to have
#[case::positive_create(Case { description: "a user creates a private theme at version 0", setup: &[], action: Put(Alice, "blue", same), expected: Expect::Ok })]
#[case::positive_update_own(Case { description: "the owner updates at the stored version", setup: &[Put(Alice, "blue", same)], action: Put(Alice, "blue", v1), expected: Expect::Ok })]
#[case::positive_get_own(Case { description: "the owner reads their private theme", setup: &[Put(Alice, "blue", same)], action: Get(Alice, "blue"), expected: Expect::Ok })]
#[case::positive_get_builtin(Case { description: "everyone reads a built-in", setup: &[], action: Get(AliceElsewhere, "tn3270"), expected: Expect::Ok })]
#[case::positive_publisher_shares(Case { description: "a publisher shares a theme with the tenant", setup: &[], action: Put(Admin, "team", shared), expected: Expect::Ok })]
#[case::positive_tenant_reads_shared(Case { description: "the tenant reads a shared theme", setup: &[Put(Admin, "team", shared)], action: Get(Bob, "team"), expected: Expect::Ok })]
#[case::positive_delete_own(Case { description: "the owner deletes their private theme", setup: &[Put(Alice, "blue", same)], action: Delete(Alice, "blue"), expected: Expect::Ok })]
#[case::positive_publisher_deletes_shared(Case { description: "a publisher deletes a shared theme someone else made", setup: &[Put(Admin, "team", shared)], action: Delete(Admin, "team"), expected: Expect::Ok })]
#[case::positive_pick_builtin(Case { description: "anyone picks a built-in", setup: &[], action: SetActive(Bob, "tn3270", pb::ThemeMode::Dark), expected: Expect::Ok })]
#[case::positive_pick_shared(Case { description: "a tenant member picks a shared theme", setup: &[Put(Admin, "team", shared)], action: SetActive(Bob, "team", pb::ThemeMode::System), expected: Expect::Ok })]
#[case::positive_pick_own_private(Case { description: "the owner picks their private theme", setup: &[Put(Alice, "blue", same)], action: SetActive(Alice, "blue", pb::ThemeMode::Light), expected: Expect::Ok })]
// negative: plain refusals
#[case::negative_get_missing(Case { description: "an id nobody stored is missing", setup: &[], action: Get(Alice, "nope"), expected: Expect::NotFound })]
#[case::negative_delete_missing(Case { description: "deleting an id nobody stored is missing", setup: &[], action: Delete(Alice, "nope"), expected: Expect::NotFound })]
#[case::negative_put_builtin_id(Case { description: "even a publisher may not overwrite a built-in", setup: &[], action: Put(Admin, "windows", same), expected: Expect::Denied })]
#[case::negative_delete_builtin(Case { description: "even a publisher may not delete a built-in", setup: &[], action: Delete(Admin, "prototype"), expected: Expect::Denied })]
#[case::negative_stale_version(Case { description: "an update at an old version conflicts", setup: &[Put(Alice, "blue", same), Put(Alice, "blue", v1)], action: Put(Alice, "blue", v1), expected: Expect::Conflict })]
#[case::negative_recreate_at_zero(Case { description: "version 0 on a stored theme conflicts, not overwrites", setup: &[Put(Alice, "blue", same)], action: Put(Alice, "blue", same), expected: Expect::Conflict })]
#[case::negative_create_at_nonzero(Case { description: "creating at a made-up version conflicts", setup: &[], action: Put(Alice, "blue", v5), expected: Expect::Conflict })]
#[case::negative_share_without_publish(Case { description: "sharing needs (approve, theme)", setup: &[], action: Put(Alice, "blue", shared), expected: Expect::Denied })]
#[case::negative_member_edits_shared(Case { description: "a member may not edit a shared theme", setup: &[Put(Admin, "team", shared)], action: Put(Bob, "team", shared_v1), expected: Expect::Denied })]
#[case::negative_member_unshares(Case { description: "a member may not take a shared theme private", setup: &[Put(Admin, "team", shared)], action: Put(Bob, "team", v1), expected: Expect::Denied })]
#[case::negative_member_deletes_shared(Case { description: "a member may not delete a shared theme", setup: &[Put(Admin, "team", shared)], action: Delete(Bob, "team"), expected: Expect::Denied })]
#[case::negative_invalid_theme(Case { description: "the validator runs on Put", setup: &[], action: Put(Alice, "blue", radius_33), expected: Expect::Invalid(ThemeErrorKind::OutOfRange) })]
#[case::negative_pick_missing(Case { description: "picking a theme nobody stored is missing", setup: &[], action: SetActive(Alice, "nope", pb::ThemeMode::System), expected: Expect::NotFound })]
#[case::negative_bad_mode(Case { description: "a mode outside ThemeMode is refused", setup: &[], action: SetActiveRaw(Alice, "prototype", 99), expected: Expect::Invalid(ThemeErrorKind::OutOfRange) })]
// boundary: the per-tenant cap
#[case::boundary_last_slot(Case { description: "the 200th theme fits", setup: &[Fill(Alice, MAX_THEMES_PER_TENANT - 1)], action: Put(Alice, "one-more", same), expected: Expect::Ok })]
#[case::boundary_full(Case { description: "the 201st theme is refused", setup: &[Fill(Alice, MAX_THEMES_PER_TENANT)], action: Put(Alice, "one-more", same), expected: Expect::Full })]
#[case::boundary_full_counts_the_tenant(Case { description: "the cap is per tenant, not per user", setup: &[Fill(Alice, MAX_THEMES_PER_TENANT)], action: Put(Bob, "bobs", same), expected: Expect::Full })]
#[case::boundary_update_when_full(Case { description: "updating a stored theme needs no free slot", setup: &[Fill(Alice, MAX_THEMES_PER_TENANT)], action: Put(Alice, "fill-000", v1), expected: Expect::Ok })]
#[case::boundary_cap_is_per_tenant(Case { description: "a full tenant does not fill another", setup: &[Fill(Alice, MAX_THEMES_PER_TENANT)], action: Put(AliceElsewhere, "one-more", same), expected: Expect::Ok })]
#[case::boundary_bad_mode_negative(Case { description: "a negative mode is refused", setup: &[], action: SetActiveRaw(Alice, "prototype", -1), expected: Expect::Invalid(ThemeErrorKind::OutOfRange) })]
#[case::boundary_last_mode(Case { description: "the highest ThemeMode is accepted", setup: &[], action: SetActiveRaw(Alice, "prototype", pb::ThemeMode::Dark as i32), expected: Expect::Ok })]
// corner
#[case::corner_recreate_after_delete(Case { description: "a deleted id can be created again at version 0", setup: &[Put(Alice, "blue", same), Delete(Alice, "blue")], action: Put(Alice, "blue", same), expected: Expect::Ok })]
#[case::corner_get_after_delete(Case { description: "a deleted theme is missing", setup: &[Put(Alice, "blue", same), Delete(Alice, "blue")], action: Get(Alice, "blue"), expected: Expect::NotFound })]
#[case::corner_publisher_unshares(Case { description: "a publisher may take a shared theme private again", setup: &[Put(Admin, "team", shared)], action: Put(Admin, "team", v1), expected: Expect::Ok })]
#[case::corner_unshared_hidden(Case { description: "a theme taken private is hidden from the tenant again", setup: &[Put(Admin, "team", shared), Put(Admin, "team", v1)], action: Get(Bob, "team"), expected: Expect::NotFound })]
#[case::corner_pick_unspecified_mode(Case { description: "mode 0 (unspecified) is accepted and means system", setup: &[], action: SetActiveRaw(Alice, "macos", 0), expected: Expect::Ok })]
#[case::corner_same_id_two_tenants(Case { description: "two tenants may use the same id", setup: &[Put(Alice, "blue", same)], action: Put(AliceElsewhere, "blue", same), expected: Expect::Ok })]
// adversarial: another user, another tenant, hostile ids and fields
#[case::adversarial_read_private(Case { description: "another user's private theme is missing, not forbidden", setup: &[Put(Alice, "blue", same)], action: Get(Bob, "blue"), expected: Expect::NotFound })]
#[case::adversarial_delete_private(Case { description: "deleting another user's private theme is missing", setup: &[Put(Alice, "blue", same)], action: Delete(Bob, "blue"), expected: Expect::NotFound })]
#[case::adversarial_overwrite_private(Case { description: "overwriting another user's private theme is refused", setup: &[Put(Alice, "blue", same)], action: Put(Bob, "blue", v1), expected: Expect::Denied })]
#[case::adversarial_publisher_overwrite_private(Case { description: "a publisher may not overwrite someone's private theme either", setup: &[Put(Alice, "blue", same)], action: Put(Admin, "blue", shared_v1), expected: Expect::Denied })]
#[case::adversarial_pick_private(Case { description: "picking another user's private theme is missing", setup: &[Put(Alice, "blue", same)], action: SetActive(Bob, "blue", pb::ThemeMode::System), expected: Expect::NotFound })]
#[case::adversarial_other_tenant_shared(Case { description: "another tenant's shared theme is missing", setup: &[Put(Admin, "team", shared)], action: Get(AliceElsewhere, "team"), expected: Expect::NotFound })]
#[case::adversarial_same_subject_other_tenant(Case { description: "the same subject in another tenant does not see the private theme", setup: &[Put(Alice, "blue", same)], action: Get(AliceElsewhere, "blue"), expected: Expect::NotFound })]
#[case::adversarial_pick_other_tenant(Case { description: "picking another tenant's shared theme is missing", setup: &[Put(Admin, "team", shared)], action: SetActive(AliceElsewhere, "team", pb::ThemeMode::System), expected: Expect::NotFound })]
#[case::adversarial_traversal_put(Case { description: "a traversal id is refused on Put", setup: &[], action: Put(Alice, "../../etc", same), expected: Expect::Invalid(ThemeErrorKind::BadId) })]
#[case::adversarial_traversal_get(Case { description: "a traversal id reads nothing", setup: &[Put(Alice, "blue", same)], action: Get(AliceElsewhere, "../acme/blue"), expected: Expect::NotFound })]
#[case::adversarial_traversal_delete(Case { description: "a traversal id deletes nothing", setup: &[], action: Delete(Alice, "../x"), expected: Expect::NotFound })]
#[case::adversarial_traversal_pick(Case { description: "a traversal id cannot be picked", setup: &[], action: SetActive(Alice, "../x", pb::ThemeMode::System), expected: Expect::NotFound })]
#[case::adversarial_empty_id(Case { description: "an empty id is refused", setup: &[], action: Put(Alice, "", same), expected: Expect::Invalid(ThemeErrorKind::BadId) })]
#[case::adversarial_lookalike_id(Case { description: "an id that reads as a built-in is refused", setup: &[], action: Put(Alice, "w1ndows", same), expected: Expect::Invalid(ThemeErrorKind::BadName) })]
#[case::adversarial_unknown_visibility(Case { description: "a visibility outside the enum is refused, not read as private", setup: &[], action: Put(Alice, "blue", visibility_99), expected: Expect::Invalid(ThemeErrorKind::OutOfRange) })]
#[case::adversarial_hostile_fields(Case { description: "builtin and owner from the client are ignored, not refused", setup: &[], action: Put(Alice, "blue", hostile_fields), expected: Expect::Ok })]
#[tokio::test]
async fn book_table(#[case] c: Case) {
    let b = book(FALLBACK_THEME);
    setup(&b, c.setup).await;
    assert_eq!(run(&b, c.action).await, c.expected, "{}", c.description);
}

/// One row of the active-theme fallback chain (04-per-user-active-theme.md).
struct ActiveCase {
    description: &'static str,
    default_theme: &'static str,
    setup: &'static [Step],
    who: Who,
    expected: (pb::ActiveSource, &'static str, pb::ThemeMode),
}

#[rstest]
#[case::positive_user_pick(ActiveCase { description: "the user's pick wins", default_theme: "windows", setup: &[SetActive(Alice, "tn3270", pb::ThemeMode::Dark)], who: Alice, expected: (pb::ActiveSource::User, "tn3270", pb::ThemeMode::Dark) })]
#[case::positive_own_theme_pick(ActiveCase { description: "a user theme can be the pick", default_theme: "windows", setup: &[Put(Alice, "blue", same), SetActive(Alice, "blue", pb::ThemeMode::Light)], who: Alice, expected: (pb::ActiveSource::User, "blue", pb::ThemeMode::Light) })]
#[case::positive_no_pick(ActiveCase { description: "no pick falls to the server default", default_theme: "windows", setup: &[], who: Alice, expected: (pb::ActiveSource::ServerDefault, "windows", pb::ThemeMode::Unspecified) })]
#[case::negative_default_names_nothing(ActiveCase { description: "a default that names nothing falls to prototype", default_theme: "nope", setup: &[], who: Alice, expected: (pb::ActiveSource::Builtin, "prototype", pb::ThemeMode::Unspecified) })]
#[case::corner_pick_deleted(ActiveCase { description: "a deleted pick falls to the default, keeping the mode", default_theme: "macos", setup: &[Put(Alice, "blue", same), SetActive(Alice, "blue", pb::ThemeMode::Light), Delete(Alice, "blue")], who: Alice, expected: (pb::ActiveSource::ServerDefault, "macos", pb::ThemeMode::Light) })]
#[case::corner_pick_unshared(ActiveCase { description: "a pick taken private by its publisher falls to the default", default_theme: "macos", setup: &[Put(Admin, "team", shared), SetActive(Bob, "team", pb::ThemeMode::Dark), Put(Admin, "team", v1)], who: Bob, expected: (pb::ActiveSource::ServerDefault, "macos", pb::ThemeMode::Dark) })]
#[case::corner_default_is_shared_theme(ActiveCase { description: "the default may be a tenant's shared theme", default_theme: "team", setup: &[Put(Admin, "team", shared)], who: Bob, expected: (pb::ActiveSource::ServerDefault, "team", pb::ThemeMode::Unspecified) })]
#[case::corner_pick_overwritten(ActiveCase { description: "a second pick replaces the first", default_theme: "windows", setup: &[SetActive(Alice, "tn3270", pb::ThemeMode::Dark), SetActive(Alice, "macos", pb::ThemeMode::Light)], who: Alice, expected: (pb::ActiveSource::User, "macos", pb::ThemeMode::Light) })]
#[case::boundary_default_is_fallback(ActiveCase { description: "a default of prototype answers as the server default", default_theme: "prototype", setup: &[], who: Alice, expected: (pb::ActiveSource::ServerDefault, "prototype", pb::ThemeMode::Unspecified) })]
#[case::adversarial_default_private(ActiveCase { description: "a default naming someone's private theme does not show it to others", default_theme: "blue", setup: &[Put(Alice, "blue", same)], who: Bob, expected: (pb::ActiveSource::Builtin, "prototype", pb::ThemeMode::Unspecified) })]
#[case::adversarial_default_traversal(ActiveCase { description: "a traversal default resolves to nothing", default_theme: "../x", setup: &[], who: Alice, expected: (pb::ActiveSource::Builtin, "prototype", pb::ThemeMode::Unspecified) })]
#[case::adversarial_pick_other_tenant(ActiveCase { description: "a pick in one tenant is not the same subject's pick in another", default_theme: "windows", setup: &[SetActive(Alice, "tn3270", pb::ThemeMode::Dark)], who: AliceElsewhere, expected: (pb::ActiveSource::ServerDefault, "windows", pb::ThemeMode::Unspecified) })]
#[case::adversarial_pick_other_user(ActiveCase { description: "one user's pick is not another's", default_theme: "windows", setup: &[SetActive(Alice, "tn3270", pb::ThemeMode::Dark)], who: Bob, expected: (pb::ActiveSource::ServerDefault, "windows", pb::ThemeMode::Unspecified) })]
#[tokio::test]
async fn active_table(#[case] c: ActiveCase) {
    let b = book(c.default_theme);
    setup(&b, c.setup).await;
    let got = b.get_active(&caller(c.who)).await.expect("get_active");
    let (source, id, mode) = c.expected;
    assert_eq!(
        (got.source, got.theme.map(|t| t.id), got.mode),
        (source as i32, Some(id.to_string()), mode as i32),
        "{}",
        c.description
    );
}

#[tokio::test]
async fn positive_put_stamps_owner_and_version() {
    // The server, not the client, sets builtin, owner and version.
    let b = book(FALLBACK_THEME);
    let mut t = user_theme("blue");
    hostile_fields(&mut t);
    let stored = b.put(&caller(Alice), t).await.unwrap();
    assert_eq!(
        (stored.builtin, stored.owner.as_str(), stored.version),
        (false, "alice", 1)
    );
    let mut next = stored.clone();
    next.owner = "bob".into();
    let stored = b.put(&caller(Alice), next).await.unwrap();
    assert_eq!((stored.owner.as_str(), stored.version), ("alice", 2));
}

#[tokio::test]
async fn adversarial_owner_hidden_from_others() {
    // A shared theme shows its `author`, not who stored it.
    let b = book(FALLBACK_THEME);
    let mut t = user_theme("team");
    shared(&mut t);
    b.put(&caller(Admin), t).await.unwrap();
    assert_eq!(b.get(&caller(Admin), "team").await.unwrap().owner, "admin");
    assert_eq!(b.get(&caller(Bob), "team").await.unwrap().owner, "");
    let listed = b.list(&caller(Bob)).await.unwrap();
    let team = listed.iter().find(|t| t.id == "team").unwrap();
    assert_eq!(team.owner, "");
}

#[tokio::test]
async fn positive_list_is_builtins_then_visible_by_id() {
    let b = book(FALLBACK_THEME);
    let mut team = user_theme("team");
    shared(&mut team);
    b.put(&caller(Admin), team).await.unwrap();
    b.put(&caller(Alice), user_theme("zebra")).await.unwrap();
    b.put(&caller(Alice), user_theme("apple")).await.unwrap();
    b.put(&caller(AliceElsewhere), user_theme("elsewhere"))
        .await
        .unwrap();
    let ids = |v: Vec<pb::Theme>| v.into_iter().map(|t| t.id).collect::<Vec<_>>();
    let builtin_ids = ids(builtins().unwrap());
    let with = |extra: &[&str]| {
        let mut v = builtin_ids.clone();
        v.extend(extra.iter().map(ToString::to_string));
        v
    };
    assert_eq!(
        ids(b.list(&caller(Alice)).await.unwrap()),
        with(&["apple", "team", "zebra"])
    );
    assert_eq!(ids(b.list(&caller(Bob)).await.unwrap()), with(&["team"]));
    assert_eq!(
        ids(b.list(&caller(AliceElsewhere)).await.unwrap()),
        with(&["elsewhere"])
    );
}

#[tokio::test]
async fn corner_concurrent_creates_one_wins() {
    // Two creates of the same id at version 0: the second sees the first's write.
    let b = book(FALLBACK_THEME);
    let alice = caller(Alice);
    let (x, y) = tokio::join!(
        b.put(&alice, user_theme("blue")),
        b.put(&alice, user_theme("blue")),
    );
    let mut got = [outcome(x), outcome(y)];
    got.sort_by_key(|e| format!("{e:?}"));
    assert_eq!(got, [Expect::Conflict, Expect::Ok]);
}

struct PrefsCase {
    description: &'static str,
    subject: &'static str,
}

#[rstest]
#[case::positive_plain(PrefsCase { description: "an OIDC sub", subject: "248289761001" })]
#[case::corner_email(PrefsCase { description: "an email with @ and dots", subject: "a.b@example.com" })]
#[case::boundary_empty(PrefsCase { description: "an empty subject still hashes", subject: "" })]
#[case::adversarial_traversal(PrefsCase { description: "a traversal subject becomes plain hex", subject: "../../acme/user_prefs/x" })]
#[case::adversarial_control(PrefsCase { description: "control and bidi characters become plain hex", subject: "a\n\u{202e}b" })]
fn prefs_id_is_a_safe_segment(#[case] c: PrefsCase) {
    let id = prefs_id(c.subject);
    assert_eq!(id.len(), 32, "{}", c.description);
    assert!(
        id.bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
        "{}",
        c.description
    );
    assert!(agent_core::safe_segment(&id), "{}", c.description);
    assert_ne!(
        id,
        prefs_id(&format!("{}x", c.subject)),
        "{}",
        c.description
    );
}
