//! Shared test helper: read the `(google.api.http)` REST annotations out of the
//! emitted `FILE_DESCRIPTOR_SET` (gap-analysis §4, docs/design/rest-openapi/).
//!
//! `prost` silently drops unknown fields on decode, and `prost_types::MethodOptions`
//! has no field for the custom `google.api.http` extension (field 72295728) — so we
//! decode the descriptor with a *minimal* mirror of the relevant protobuf messages
//! that declares exactly that extension field. This reads the option without pulling
//! in a dynamic-reflection dependency (e.g. `prost-reflect`).
//!
//! Used by `http_annotations.rs` (the per-RPC annotation manifest + whole-set
//! invariants) and `openapi_parity.rs` (the committed OpenAPI doc must cover exactly
//! these routes). Both derive the route set from THIS one decoder, so they can never
//! disagree about what the descriptor says.

// Not every consumer touches every field/helper here; each test binary includes the
// whole module. This is the shared-`tests/common/mod.rs` idiom.
#![allow(dead_code)]

use agent_proto::FILE_DESCRIPTOR_SET;
use prost::Message;
use std::collections::BTreeMap;

#[derive(Clone, PartialEq, Message)]
struct FileDescriptorSet {
    #[prost(message, repeated, tag = "1")]
    file: Vec<FileDescriptorProto>,
}

#[derive(Clone, PartialEq, Message)]
struct FileDescriptorProto {
    #[prost(message, repeated, tag = "6")]
    service: Vec<ServiceDescriptorProto>,
}

#[derive(Clone, PartialEq, Message)]
struct ServiceDescriptorProto {
    #[prost(string, optional, tag = "1")]
    name: Option<String>,
    #[prost(message, repeated, tag = "2")]
    method: Vec<MethodDescriptorProto>,
}

#[derive(Clone, PartialEq, Message)]
struct MethodDescriptorProto {
    #[prost(string, optional, tag = "1")]
    name: Option<String>,
    #[prost(bool, optional, tag = "5")]
    client_streaming: Option<bool>,
    #[prost(bool, optional, tag = "6")]
    server_streaming: Option<bool>,
    #[prost(message, optional, tag = "4")]
    options: Option<MethodOptions>,
}

// `google.protobuf.MethodOptions` carries the `(google.api.http)` extension at field
// 72295728 (declared in the vendored `google/api/annotations.proto`). We mirror only
// that one field; every other MethodOptions field is skipped on decode.
#[derive(Clone, PartialEq, Message)]
struct MethodOptions {
    #[prost(message, optional, tag = "72295728")]
    http: Option<HttpRule>,
}

// `google.api.HttpRule` (subset: the pattern verbs, body, and one level of bindings).
#[derive(Clone, PartialEq, Message)]
struct HttpRule {
    #[prost(string, optional, tag = "2")]
    get: Option<String>,
    #[prost(string, optional, tag = "3")]
    put: Option<String>,
    #[prost(string, optional, tag = "4")]
    post: Option<String>,
    #[prost(string, optional, tag = "5")]
    delete: Option<String>,
    #[prost(string, optional, tag = "6")]
    patch: Option<String>,
    #[prost(string, optional, tag = "7")]
    body: Option<String>,
    #[prost(message, repeated, tag = "11")]
    additional_bindings: Vec<HttpRule>,
}

/// One transcoding route: an HTTP verb + a path template (`/v1/…/{id}`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    pub verb: &'static str,
    pub path: String,
}

impl HttpRule {
    fn primary_route(&self) -> Option<Route> {
        let (verb, path) = if let Some(p) = &self.get {
            ("GET", p)
        } else if let Some(p) = &self.post {
            ("POST", p)
        } else if let Some(p) = &self.put {
            ("PUT", p)
        } else if let Some(p) = &self.delete {
            ("DELETE", p)
        } else if let Some(p) = &self.patch {
            ("PATCH", p)
        } else {
            return None;
        };
        Some(Route {
            verb,
            path: path.clone(),
        })
    }

    /// The primary binding plus any `additional_bindings` (one level deep).
    fn routes(&self) -> Vec<Route> {
        let mut out = Vec::new();
        out.extend(self.primary_route());
        for b in &self.additional_bindings {
            out.extend(b.primary_route());
        }
        out
    }
}

/// A decoded RPC: its streaming kind + the transcoding routes it declares.
pub struct MethodInfo {
    pub client_streaming: bool,
    pub routes: Vec<Route>,
}

/// Decode the emitted descriptor set into `"Service.Method" -> MethodInfo`.
pub fn methods() -> BTreeMap<String, MethodInfo> {
    let set =
        FileDescriptorSet::decode(FILE_DESCRIPTOR_SET).expect("emitted descriptor set decodes");
    let mut map = BTreeMap::new();
    for f in &set.file {
        for s in &f.service {
            let Some(svc) = s.name.as_deref() else {
                continue;
            };
            for m in &s.method {
                let Some(name) = m.name.as_deref() else {
                    continue;
                };
                let routes = m
                    .options
                    .as_ref()
                    .and_then(|o| o.http.as_ref())
                    .map(HttpRule::routes)
                    .unwrap_or_default();
                map.insert(
                    format!("{svc}.{name}"),
                    MethodInfo {
                        client_streaming: m.client_streaming.unwrap_or(false),
                        routes,
                    },
                );
            }
        }
    }
    map
}
