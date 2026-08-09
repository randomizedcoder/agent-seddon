//! Textproto encode/decode for the graph document: the human-readable form of
//! `agent.v1.CognitionGraph`, parsed with `prost-reflect` over the **same
//! descriptor set** `agent-proto` already emits for gRPC reflection — one
//! schema, three encodings (binary wire, text file, Rust structs), nothing to
//! drift.
//!
//! Fail closed: a size cap applies **before** any parsing work (textproto
//! bombs), parse errors carry the parser's line/column detail, and the decoded
//! document still goes through [`crate::validate`] before anything trusts it —
//! parsing here proves *shape*, not *meaning*.

use agent_core::{Error, GraphDoc, Result, MAX_GRAPH_DOC_BYTES};
use agent_proto::pb;
use prost_reflect::text_format::FormatOptions;
use prost_reflect::{DescriptorPool, DynamicMessage, MessageDescriptor};
use std::sync::OnceLock;

fn graph_descriptor() -> MessageDescriptor {
    static POOL: OnceLock<DescriptorPool> = OnceLock::new();
    POOL.get_or_init(|| {
        DescriptorPool::decode(agent_proto::FILE_DESCRIPTOR_SET)
            .expect("emitted descriptor set decodes")
    })
    .get_message_by_name("agent.v1.CognitionGraph")
    .expect("CognitionGraph is compiled into the descriptor set")
}

/// Parse a textproto document into a [`GraphDoc`]. Size-capped before parse;
/// unknown edge kinds are rejected at the wire→core conversion (fail closed).
pub fn parse(text: &str) -> Result<GraphDoc> {
    if text.len() > MAX_GRAPH_DOC_BYTES {
        return Err(Error::Graph(format!(
            "graph document is {} bytes (cap {MAX_GRAPH_DOC_BYTES}) — refusing to parse",
            text.len()
        )));
    }
    let msg = DynamicMessage::parse_text_format(graph_descriptor(), text)
        .map_err(|e| Error::Graph(format!("textproto parse: {e}")))?;
    let wire: pb::CognitionGraph = msg
        .transcode_to()
        .map_err(|e| Error::Graph(format!("textproto transcode: {e}")))?;
    wire.try_into()
        .map_err(|e| Error::Graph(format!("graph decode: {e}")))
}

/// Print a document as pretty textproto (the stored / diffable form).
pub fn print(doc: &GraphDoc) -> Result<String> {
    let wire: pb::CognitionGraph = doc.clone().into();
    let mut msg = DynamicMessage::new(graph_descriptor());
    msg.transcode_from(&wire)
        .map_err(|e| Error::Graph(format!("textproto encode: {e}")))?;
    Ok(msg.to_text_format_with_options(&FormatOptions::new().pretty(true)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdata;
    use rstest::rstest;

    #[rstest]
    #[case::simple(testdata::simple())]
    #[case::intermediate(testdata::intermediate())]
    fn positive_print_parse_roundtrips(#[case] doc: GraphDoc) {
        let text = print(&doc).expect("print");
        let back = parse(&text).expect("parse back");
        assert_eq!(back, doc);
    }

    #[test]
    fn positive_hand_written_document_parses() {
        // The exact style a user writes (and the design doc shows).
        let text = r#"
            version: 1
            nodes {
              key: "generate"
              value { type: "generate" type_version: 1 }
            }
            nodes {
              key: "gate"
              value {
                type: "critic_gate"
                type_version: 1
                params {
                  object_value {
                    fields { key: "critic" value { string_value: "glm" } }
                    fields { key: "max_rounds" value { uint_value: 2 } }
                  }
                }
              }
            }
            edges { from: "anchor.response" to: "generate" kind: KIND_MAIN }
            edges { from: "generate" to: "gate" kind: KIND_MAIN }
        "#;
        let doc = parse(text).expect("parses");
        assert_eq!(doc.version, 1);
        assert_eq!(doc.nodes.len(), 2);
        assert_eq!(doc.nodes["gate"].params["max_rounds"], 2);
        assert_eq!(doc.edges.len(), 2);
    }

    #[rstest]
    #[case::garbage("this is not a textproto {{{")]
    #[case::wrong_field("version: 1 bogus_field: \"x\"")]
    #[case::type_mismatch("version: \"one\"")]
    fn negative_malformed_text_rejected(#[case] text: &str) {
        assert!(parse(text).is_err());
    }

    #[test]
    fn corner_empty_text_is_an_empty_document() {
        // All-defaults textproto: version 0 — parses (shape), then fails
        // validation (meaning). The layers stay distinct.
        let doc = parse("").expect("empty text is a valid (default) message");
        assert_eq!(doc, GraphDoc::default());
    }

    #[test]
    fn adversarial_textproto_bomb_refused_before_parse() {
        let bomb = testdata::textproto_bomb();
        let err = parse(&bomb).expect_err("size cap");
        assert!(err.to_string().contains("refusing to parse"), "{err}");
    }

    #[test]
    fn adversarial_unspecified_edge_kind_rejected() {
        // Kind absent ⇒ KIND_UNSPECIFIED ⇒ the conversion refuses it.
        let text = r#"
            version: 1
            nodes { key: "generate" value { type: "generate" type_version: 1 } }
            edges { from: "anchor.response" to: "generate" }
        "#;
        let err = parse(text).expect_err("unspecified kind");
        assert!(err.to_string().contains("GraphEdge.kind"), "{err}");
    }
}
