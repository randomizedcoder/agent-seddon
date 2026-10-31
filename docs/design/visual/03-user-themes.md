# 03 — User themes

Most theming systems let users make their own, and the people who care about design make
the best ones. A user theme is a `Theme` document ([`01`](01-theme-model.md)) that a
signed-in user creates in the portal, keeps private or shares with their tenant.

## Authoring

Three ways in, all producing the same document:

1. **Clone and edit.** In the portal's theme editor (increment 4), pick any theme, clone
   it, and change tokens with colour pickers and font dropdowns. A live preview pane
   shows the Launch, Fleet and a form page in the draft theme, and each colour pair shows
   its contrast ratio and a pass or fail badge as you edit.
2. **Import a file.** Paste or upload a `.textproto` (or proto3 JSON). The server
   validates it exactly like an editor save.
3. **In the repo.** Operators can add `config/themes/<id>.textproto` and ship it as a
   built-in through a normal PR.

Export gives back the textproto, so a theme can be shared in a chat or committed.

## `ThemeService`

Added to `theme.proto`, mounted on the gateway (`--serve-all`) next to `PromptService`,
with `google.api.http` annotations so the REST transcoder exposes it too.

```proto
service ThemeService {
  rpc List(ListThemesRequest) returns (ListThemesResponse) {
    option (google.api.http) = { get: "/v1/themes" };
  }
  rpc Get(GetThemeRequest) returns (Theme) {
    option (google.api.http) = { get: "/v1/themes/{id}" };
  }
  rpc Put(Theme) returns (Theme) {                 // upsert; returns the stored theme
    option (google.api.http) = { put: "/v1/themes/{id}" body: "*" };
  }
  rpc Delete(DeleteThemeRequest) returns (DeleteThemeResponse) {
    option (google.api.http) = { delete: "/v1/themes/{id}" };
  }
  rpc GetActive(GetActiveThemeRequest) returns (ActiveTheme) {        // see 04
    option (google.api.http) = { get: "/v1/themes:active" };
  }
  rpc SetActive(SetActiveThemeRequest) returns (ActiveTheme) {
    option (google.api.http) = { put: "/v1/themes:active" body: "*" };
  }
}

message ListThemesRequest {}   // what the caller may see: built-ins, tenant themes, own private
message ListThemesResponse { repeated Theme themes = 1; }
message GetThemeRequest { string id = 1; }
message DeleteThemeRequest { string id = 1; }
message DeleteThemeResponse { bool restored_builtin = 1; }  // deleting an override brings the built-in back
```

`List` returns full theme documents: a theme is capped at 64 KiB
([`06`](06-security-and-validation.md)) and a tenant at 200 themes, so a full list is
bounded.

## Storage

> **As built (3a):** built-in overrides are not implemented yet (built-in ids are
> refused), `publish:theme` is `(approve, theme)`, the active-theme route is
> `/v1/active-theme`, the backends are `file` and `postgres`, and private theme ids
> are per person (200 shared themes per tenant, 20 private per person). See
> [STATUS](STATUS.md#3a--themeservice-store-rbac).

Themes are cards in `agent-config-store`, collection `themes`, keyed by
`(tenant, id)`, exactly like prompts (`crates/agent-prompt/src/store.rs`):

- Backends: `file`, `sqlite`, `postgres`, chosen by `[themes] store`. The NixOS module
  sets `postgres`, like the other transactional stores.
- **Built-ins** are compiled in from `config/themes/*.textproto` (`include_str!`). `List`
  merges them with stored cards; a stored card with a built-in's id is an **override**
  (operators only, `write:theme_builtin`), and `Delete` of it restores the built-in.
- `version` increments on every `Put`, and `Put` with a stale `version` fails with
  `ABORTED` so two editors cannot silently overwrite each other.
- Per-tenant: the store is wrapped per tenant when `[tenancy] per_tenant` is on, so one
  tenant never sees another's themes.

## Sharing and permissions

| Visibility | Who sees it | Who may create or edit it |
|---|---|---|
| built-in | everyone | operators, via a repo PR or an override |
| `VISIBILITY_TENANT` | everyone in the tenant | holders of `publish:theme` |
| `VISIBILITY_PRIVATE` | the owner | the owner (`write:theme`) |

The server sets `owner` from the caller's verified identity and ignores any `owner` the
client sends. Ids are unique per tenant, so a private theme cannot shadow a tenant or
built-in theme's id; `Put` of an id owned by someone else fails with `PERMISSION_DENIED`
rather than revealing that the id exists.

New RBAC capabilities: `read:theme` (granted to every authenticated role), `write:theme`,
`publish:theme`, `write:theme_builtin`. They follow the existing `read:forge_registry`
naming and go through `agent_core::authorize` like every other RPC.

## Why not a public gallery

A cross-tenant gallery means one tenant's content rendering in another tenant's session,
which needs moderation and a trust model this track does not build. Export and import
cover sharing between tenants for now.
