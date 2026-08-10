//! Textproto encode/decode for the model-router config: the human-readable form
//! of `agent.v1.ModelRouterConfig`, parsed with `prost-reflect` over the **same
//! descriptor set** `agent-proto` already emits for gRPC reflection — one
//! schema, three encodings (binary wire, text file, Rust structs), nothing to
//! drift. Mirrors the cognition graph's textproto module.
//!
//! Fail closed: a size cap applies **before** any parsing work (textproto
//! bombs), parse errors carry the parser's line/column detail, and the decoded
//! config still goes through [`ModelRouterConfig::validate`] in every caller
//! (store load / startup loader) before anything trusts it — parsing here
//! proves *shape* (plus the decode-time number clamps), not *meaning*.

use agent_core::{Error, ModelRouterConfig, Result, MAX_MODEL_ROUTER_CONFIG_BYTES};
use agent_proto::pb;
use prost_reflect::text_format::FormatOptions;
use prost_reflect::{DescriptorPool, DynamicMessage, MessageDescriptor};
use std::sync::OnceLock;

fn config_descriptor() -> MessageDescriptor {
    static POOL: OnceLock<DescriptorPool> = OnceLock::new();
    POOL.get_or_init(|| {
        DescriptorPool::decode(agent_proto::FILE_DESCRIPTOR_SET)
            .expect("emitted descriptor set decodes")
    })
    .get_message_by_name("agent.v1.ModelRouterConfig")
    .expect("ModelRouterConfig is compiled into the descriptor set")
}

/// Parse a textproto config. Size-capped before parse; numbers are clamped by
/// the wire→core decode. Callers must still `validate()` (fail closed) before
/// loading the result anywhere.
pub fn parse(text: &str) -> Result<ModelRouterConfig> {
    if text.len() > MAX_MODEL_ROUTER_CONFIG_BYTES {
        return Err(Error::Registry(format!(
            "model-router config is {} bytes (cap {MAX_MODEL_ROUTER_CONFIG_BYTES}) — refusing to parse",
            text.len()
        )));
    }
    let msg = DynamicMessage::parse_text_format(config_descriptor(), text)
        .map_err(|e| Error::Registry(format!("textproto parse: {e}")))?;
    let wire: pb::ModelRouterConfig = msg
        .transcode_to()
        .map_err(|e| Error::Registry(format!("textproto transcode: {e}")))?;
    Ok(wire.into())
}

/// Print a config as pretty textproto (the stored / diffable form).
pub fn print(cfg: &ModelRouterConfig) -> Result<String> {
    let wire: pb::ModelRouterConfig = cfg.clone().into();
    let mut msg = DynamicMessage::new(config_descriptor());
    msg.transcode_from(&wire)
        .map_err(|e| Error::Registry(format!("textproto encode: {e}")))?;
    Ok(msg.to_text_format_with_options(&FormatOptions::new().pretty(true)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdata;
    use rstest::rstest;

    #[test]
    fn positive_print_parse_roundtrips() {
        let cfg = testdata::config();
        let text = print(&cfg).expect("print");
        let back = parse(&text).expect("parse back");
        assert_eq!(back, cfg);
        back.validate().expect("stays valid");
    }

    #[test]
    fn positive_hand_written_config_parses() {
        // The exact style an operator writes (and the design doc shows).
        let text = r#"
            upstreams {
              id: "kimi"
              kind: "openai-compat"
              enabled: true
              base_url: "https://kimi.example/v1"
              model: "kimi-k3"
              api_key_ref: "file:~/keys/kimi"
              context_window: 262144
              supports_tools: true
              tags: "reasoning"
              tier: POOL_TIER_HEAVY
              input_cost: 0.6
            }
            upstreams {
              id: "glm"
              kind: "openai-compat"
              enabled: true
              base_url: "https://glm.example/v1"
              model: "glm-5.2"
              api_key_ref: "env:GLM_KEY"
              insecure_tls: true
            }
            policy {
              rules {
                match { role: ROUTE_ROLE_JUDGE }
                prefer { tags: "reasoning" tier: POOL_TIER_HEAVY }
              }
              default_prefer { upstreams: "kimi" upstreams: "glm" }
              failure_threshold: 3
              cooldown_secs: 30
            }
        "#;
        let cfg = parse(text).expect("parses");
        cfg.validate().expect("valid");
        assert_eq!(cfg.upstreams.len(), 2);
        assert_eq!(cfg.upstreams[0].tier, Some(agent_core::PoolTier::Heavy));
        assert_eq!(cfg.policy.rules.len(), 1);
        assert_eq!(
            cfg.policy.rules[0].match_.role,
            Some(agent_core::RouteRole::Judge)
        );
        assert_eq!(cfg.policy.default_prefer.upstreams, vec!["kimi", "glm"]);
    }

    #[rstest]
    #[case::garbage("this is not a textproto {{{")]
    #[case::wrong_field("upstreams { id: \"a\" bogus_field: 1 }")]
    #[case::type_mismatch("upstreams { id: \"a\" context_window: \"lots\" }")]
    #[case::truncated("upstreams { id: \"a\"")]
    fn negative_malformed_text_rejected(#[case] text: &str) {
        assert!(parse(text).is_err());
    }

    #[test]
    fn corner_empty_text_is_an_empty_config() {
        // All-defaults textproto parses (shape); an empty fleet is a *valid*
        // registry state (meaning) — routing over it yields "no candidate".
        let cfg = parse("").expect("empty text is a valid (default) message");
        assert_eq!(cfg, ModelRouterConfig::default());
        cfg.validate().expect("empty is valid");
    }

    #[test]
    fn adversarial_textproto_bomb_refused_before_parse() {
        let bomb = format!(
            "upstreams {{ id: \"a\" tags: \"{}\" }}",
            "x".repeat(MAX_MODEL_ROUTER_CONFIG_BYTES)
        );
        let err = parse(&bomb).expect_err("size cap");
        assert!(err.to_string().contains("refusing to parse"), "{err}");
    }

    #[test]
    fn adversarial_parsed_config_still_fails_validation_closed() {
        // Parses cleanly (shape) but must fail validate() (meaning): raw secret.
        let cfg = parse(r#"upstreams { id: "a" api_key_ref: "sk-raw-secret" }"#).expect("shape ok");
        assert!(cfg.validate().is_err());
        // Traversal id: parses, refuses validation.
        let cfg = parse(r#"upstreams { id: "../escape" }"#).expect("shape ok");
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn adversarial_hostile_numbers_clamped_at_decode() {
        let cfg = parse(
            r#"upstreams { id: "a" input_cost: nan weight: -inf context_window: 4294967295 }"#,
        )
        .expect("shape ok");
        assert_eq!(cfg.upstreams[0].input_cost, 0.0);
        assert_eq!(cfg.upstreams[0].weight, 0.0);
        assert_eq!(
            cfg.upstreams[0].context_window,
            agent_core::MAX_ROUTE_MIN_CONTEXT
        );
    }
}
