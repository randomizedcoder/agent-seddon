# 04 — Per-user active theme

Each signed-in user picks their theme and light/dark mode once, and it follows them to
any browser. The choice is stored server-side.

## The preference

```proto
enum ThemeMode {
  THEME_MODE_UNSPECIFIED = 0;  // = SYSTEM
  THEME_MODE_SYSTEM = 1;       // follow the OS / browser
  THEME_MODE_LIGHT = 2;
  THEME_MODE_DARK = 3;
}

message ActiveTheme {
  Theme theme = 1;             // the resolved document, so the portal needs one round trip
  ThemeMode mode = 2;
  ActiveSource source = 3;     // why this theme: the user's pick or a fallback
}

enum ActiveSource {
  ACTIVE_SOURCE_UNSPECIFIED = 0;
  ACTIVE_SOURCE_USER = 1;           // the caller's own preference
  ACTIVE_SOURCE_SERVER_DEFAULT = 2; // no preference, or the preferred theme is gone
  ACTIVE_SOURCE_BUILTIN = 3;        // the server default itself is missing or invalid
}

message GetActiveThemeRequest {}
message SetActiveThemeRequest { string theme_id = 1; ThemeMode mode = 2; }
```

It is stored as a `UserPrefs` card in collection `user_prefs`, keyed by the caller's
identity. A separate message keeps it extensible (density override, text scale) without
touching `Theme`.

## Whose preference

> **As built (3a):** the key is `(tenant, sha256(subject))` (the verified principal
> has no issuer), service tokens are not refused, and the server default is
> `[themes] default_theme`. See [STATUS](STATUS.md#3a--themeservice-store-rbac).

The key comes from the verified identity the auth layer installs
(`VerifiedIdentity` in `crates/agent-grpc/src/server/auth.rs`):
`(tenant, issuer, subject)`, hashed to a `safe_segment` card id. It is never read from a
request field, so one user cannot read or set another's preference.

| Situation | Key | Effect |
|---|---|---|
| Signed in (OIDC) | `(tenant, issuer, subject)` | Personal; follows the user |
| Service token | `SetActive` refused (`PERMISSION_DENIED`) | Services have no UI |
| Auth off | the host-wide `local` principal | Everyone who reaches the portal shares one choice, as with every other setting in auth-off mode |

## Resolving the theme

`GetActive` walks a fixed chain and reports which step answered:

1. The caller's `UserPrefs.theme_id`, if that theme exists **and** the caller may still
   see it (a private theme deleted, or a tenant theme unshared, drops through).
2. The server default, `[portal] default_theme` (default `"prototype"`).
3. The built-in `prototype`, which cannot be deleted.

`SetActive` checks that the caller can see `theme_id` before storing it.

## Avoiding a flash on load

The portal renders before `GetActive` returns. It keeps the last resolved theme document
in `localStorage` (a cache, not the source of truth) and paints with it, then replaces it
when the server answers. With an empty cache it paints `prototype`. The cache is keyed by
the signed-in subject, so a shared browser does not show one user's theme to the next.
