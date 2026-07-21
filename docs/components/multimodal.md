# Multimodal content

Typed content blocks on every message, so a turn can carry an image or a document
alongside prose — end to end, from `read_file` through the loop, over gRPC, and
into each provider's own wire envelope. Parity spec
[26](../parity/26-multimodal.md).

Unlike a "vision tool" bolted on the side (hermes' shape), content blocks are part
of the core `Message` contract, so **every** seam — the providers, the context
strategies, the session store, the gRPC transport — carries them without special
cases.

## The model

`agent_core::ContentBlock` is the unit of content:

```rust
pub enum ContentBlock {
    Text     { text: String },
    Image    { media_type: String, data: Vec<u8> },
    Document { media_type: String, data: Vec<u8>, name: Option<String> },
}
```

`Message.content` is a `Vec<ContentBlock>`; `Observation` keeps its `content:
String` text summary and **gains** a `blocks: Vec<ContentBlock>` field for media a
tool produced.

Helpers that keep text-only code unchanged:

| Method | Use |
|---|---|
| `Message::{system,user,assistant,tool}(s)` | unchanged — a string folds into one text block (an empty string ⇒ *no* blocks) |
| `Message::content_text()` | every text block concatenated; media contributes nothing |
| `Message::with_blocks` / `tool_with_blocks` | the multimodal constructors |
| `Message::has_media` / `strip_media(note)` | the capability gate |
| `Observation::with_blocks` / `into_blocks()` | attach media / bridge into a message |

### Serde back-compatibility

`Message` deserializes **both** a bare string and a block list:

```jsonc
{"role":"user","content":"hello"}                            // pre-spec-26 → one Text block
{"role":"user","content":[{"type":"text","text":"hello"}]}   // block list
```

This is not cosmetic: every session file, checkpoint, and gRPC JSON payload
written before this change carries the bare-string form and must keep loading.
Serialization always emits the block list; media `data` is base64 (a small
dependency-free codec in `agent-core`, matching the other hand-rolled primitives
in-tree).

> **Checkpoint hashes changed.** `agent-session` content-addresses a checkpoint by
> hashing the serialized messages, so the new serialization changes every id.
> Existing checkpoints still *load* (the serde back-compat above), but their ids
> differ from what a pre-spec-26 build computed. There is no migration; sessions
> are per-run artifacts.

## Wire contract

`common.proto` gained a `ContentBlock` message (a `text|image|document` oneof) and
the change is **purely additive**, so `buf breaking` passes untouched:

```proto
message Message {
  Role role = 1;
  string content = 2;              // ALWAYS the text — old peers still read prose
  repeated ToolCall tool_calls = 3;
  optional string tool_call_id = 4;
  repeated ContentBlock blocks = 5; // set only when NOT zero-or-one text block
}
```

The encoder sets `blocks` only when the content is not simply zero-or-one text
block, so a plain text message costs nothing extra on the wire; the decoder
prefers `blocks` and falls back to folding `content` into a single text block. A
peer built before this change keeps working — it reads the text of every message
and simply doesn't see the media. `ModelCapabilities.supports_vision` is on the
wire too, so a `= "grpc"` provider client can strip media *before* sending.

## Provider encoding

| Provider | Text | Image | Document |
|---|---|---|---|
| Anthropic | `{"type":"text",…}` | `{"type":"image","source":{"type":"base64",…}}` | `document` block for PDF, else a text note |
| OpenAI-compat | bare string (unchanged) | `{"type":"image_url","image_url":{"url":"data:…;base64,…"}}` | text note (no inline document part) |

Two rules worth knowing:

- **Empty text blocks are never sent.** The Anthropic API rejects
  `{"type":"text","text":""}`, so the encoder filters them out.
- **The OpenAI adapter keeps the bare-string form for text-only turns**, switching
  to the parts array only when media is present — text-only servers are unaffected.

## Capability gating

Sending an image to a model without vision support fails the **whole** request, so
the loop strips media first and leaves an explicit note:

```
[media omitted: the selected model does not support images]
```

- Anthropic → `supports_vision: true` (every Claude model this adapter targets).
- OpenAI-compat → **off by default**, opt in per deployment, since the endpoint is
  generic (any OpenAI-compatible server, including text-only local models):

```toml
[provider]
supports_vision = true
```

## Reading images

`read_file` on an image returns a text note **plus** an `Image` block instead of
the old "binary file … not shown" drop:

```
Read image file `diagram.png` [image/png, 20164 bytes]
```

- Detection is by **file magic**, never by extension — a `.png` holding other
  bytes stays on the binary path (the model chooses the path, so the content is
  untrusted). PNG/JPEG/GIF/WebP are recognised.
- Images over a **3 MiB** hard cap are described, not inlined (base64 inflates by
  ~4/3 and vendors cap inline media).
- Non-image binaries are unchanged.

Because `Observation` carries blocks and the loop bridges them with
`Message::tool_with_blocks`, the image reaches the *next* request intact rather
than being flattened to text.

## Token accounting

A media block is **not** text and must not be tokenized as if it were —
undercounting silently overflows the provider's window at request time.
`agent_core::media_block_tokens` is the single shared estimate (~1 token per 750
bytes, floored at 8, capped at 8000, deliberately erring high), used by all three
estimators that previously duplicated the `chars/4` heuristic:
`Tokenizer::count_messages`, `agent-context`'s `estimate_tokens`, and the
runtime's `rough_tokens`.

The summarizer renders media as `[image: image/png]` in its transcript rather than
dropping it, so a compacted history still records that the turn carried an image.

## Observability

| Metric | Labels | Meaning |
|---|---|---|
| `agent_content_blocks_total` | `modality` = `text`\|`image`\|`document` | blocks sent to the model |
| `agent_content_blocks_dropped_total` | — | media stripped for a non-vision model |

## Deferred

- **Image resize / format conversion.** pi downscales to fit inline limits and
  converts BMP→PNG; here an oversized image is described rather than re-encoded.
  Needs a decode/resize dependency (`image`), so it is a follow-up.
- **Documents** are typed and carried end to end, but only Anthropic PDFs inline;
  everything else degrades to a text note.
- **Streaming media.** `CompletionChunk.delta_text` is text-only; media arrives on
  the final message.
