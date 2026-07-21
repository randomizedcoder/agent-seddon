# Parity spec 26 — multimodal content

Per-feature parity spec for image/PDF content blocks end-to-end: `read_file` and
user attachments produce **typed** image/document blocks, and providers send them
in each vendor's multimodal message format. Tracks what agent-seddon ships today,
what the peers assert, and the concrete behaviour + tests needed to be the most
complete of the four.

> **Status: implemented** (typed `ContentBlock` on `Message`/`Observation`, both
> provider encoders, `read_file` image blocks, capability gating, block-aware
> token accounting, modality metrics, additive proto + gRPC roundtrip; doc in
> `docs/components/multimodal.md`). Two deliberate departures from the plan below:
> (1) the wire change is **purely additive** — `string content = 2` is kept as the
> always-populated text field and a `repeated ContentBlock blocks = 5` is added
> alongside, rather than retyping field 2, so `buf breaking` passes untouched and
> a pre-26 peer still reads the prose of every message; (2) `Observation` **gains**
> a `blocks` field rather than having `content` retyped, keeping its text summary
> (and every existing tool + assertion) unchanged. **Deferred:** image
> resize/downscale + format conversion (BMP→PNG), which need a decode dependency —
> an oversized image is described rather than re-encoded.
>
> Original plan follows. Introduce **typed content blocks** on
> `agent_core::Message` — `content` becomes an ordered list of `Text | Image |
> Document` blocks (back-compatible with the current bare `String`) — and mirror
> them on the wire in `crates/agent-proto/proto/agent/v1/common.proto` (agent-seddon
> already carries arbitrary JSON losslessly via `JsonValue`; this adds **typed
> binary/image blocks** so the modality survives a gRPC seam hop, not just an opaque
> blob). Providers plumb the blocks into their multimodal shapes (Anthropic
> `{type:"image",source:{type:"base64",…}}`, OpenAI `{type:"image_url",…}`), with
> **image downscale/re-encode** for per-model inline limits and **capability gating**
> (skip/degrade if the selected model is not vision-capable). Tool results (e.g. a
> screenshot) can carry image blocks too. **Differentiator:** proto-typed multimodal
> `Message` content **metered by modality** (a content-block counter labelled
> `text|image|document`) plus an OTel span over the decode/resize hot path — no peer
> exposes multimodal message content as a typed, reflection-introspectable, metered
> gRPC seam contract.

## Feature & why it matters

Screenshots, architecture diagrams, rendered charts, PDFs, and design mocks are
first-class coding inputs: "why does this UI look wrong", "implement this Figma
frame", "read the error in this screenshot", "summarise this spec PDF". A text-only
message channel is a **hard ceiling** — the model literally cannot see the artefact,
and the agent degrades to asking the user to transcribe. Every modern coding agent
therefore models message content as a **list of typed blocks**, not a string, and
teaches each provider adapter to encode image/document blocks into that vendor's
multimodal request. The interesting engineering is at the edges: detecting a binary
file as an image by content (not extension), re-encoding/downscaling to fit a
model's inline size cap, refusing gracefully when the model has no vision, and
letting a **tool result** (a browser screenshot, a rendered diagram) carry an image
back into the conversation.

## agent-seddon today

Messages are **text-only** end to end. There is no image support anywhere.

- **Core type** — [`crates/agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs):
  `Message { role: Role, content: String, tool_calls, tool_call_id }` (lines
  ~73–81). `content` is a bare `String`; the `system`/`user`/`assistant`/`tool`
  constructors all take `impl Into<String>`. `Observation { content: String,
  is_error: bool }` (~line 224) is likewise text-only, so a tool cannot return an
  image.
- **Wire contract** — [`crates/agent-proto/proto/agent/v1/common.proto`](../../crates/agent-proto/proto/agent/v1/common.proto):
  `message Message { Role role = 1; string content = 2; repeated ToolCall
  tool_calls = 3; optional string tool_call_id = 4; }` (~line 66). `JsonValue`
  (~line 36) can carry arbitrary JSON losslessly, but there is **no typed
  image/binary block** — a base64 image would ride as an opaque string with no
  modality/media-type on the wire.
- **Providers send text only** — [`crates/agent-providers/src/anthropic.rs`](../../crates/agent-providers/src/anthropic.rs)
  converts each message to content blocks but only ever emits
  `json!({ "type": "text", "text": m.content })` (~lines 347/359); tool results are
  `{type:"tool_result", content: <string>}` (~line 362). [`openai_compat.rs`](../../crates/agent-providers/src/openai_compat.rs)
  sends `content: String` directly (~line 138). Neither emits an image block.
- **`read_file` rejects binary** — [`crates/agent-tools/src/core.rs`](../../crates/agent-tools/src/core.rs):
  a NUL/invalid-UTF-8 file is reported as `` `{path}` is a binary file ({} bytes);
  not shown as text.`` (~line 141) — a clean informational message, but the bytes
  are **dropped**; a PNG never becomes model-visible content. (The 03-file-read-write
  spec explicitly flags "Image→base64 reads remain a documented follow-up (needs
  multimodal plumbing to be useful)" — this spec is that plumbing.)

Closest seams to extend: `agent-core` (`Message`/`Observation` shape), the
`LlmProvider` adapters, and the `read_file` `Tool`. No new *seam trait* is required
— multimodal is a **schema change to an existing contract** — though the
image-decode/resize helper is a natural place for a small internal utility.

## Peer implementations & their tests

| Peer     | Impl path | Test path | Framework |
| -------- | --------- | --------- | --------- |
| pi       | `pi/packages/ai/src/types.ts` (`ImageContent`), `pi/packages/coding-agent/src/core/tools/read.ts` + `pi/packages/coding-agent/src/utils/image-process.ts` (resize/convert) | `pi/packages/coding-agent/test/tools.test.ts` (read→image block), `pi/packages/ai/test/image-tool-result.test.ts`, `pi/packages/ai/test/openrouter-images.test.ts` | vitest |
| hermes   | `hermes-agent/tools/vision_tools.py` (`vision_analyze_tool`, URL→base64), `hermes-agent/tools/image_source.py`, `hermes-agent/tools/file_operations.py` (binary detect) | `hermes-agent/tests/plugins/image_gen/*` | pytest |
| opencode | `opencode/packages/core/src/session/runner/to-llm-message.ts` (`media()` FilePart → `{type:"media",mediaType,data,filename}`), read image path in `packages/core/src/tool/read.ts` | `opencode/packages/core/test/tool-read.test.ts`, `session-runner-message.test.ts` | bun:test + Effect |

**pi** is the anchor — it has the fullest typed pipeline:

- **Typed block** ([`types.ts`](../../../pi/packages/ai/src/types.ts) ~line 343):
  `interface ImageContent { type: "image"; data: string /* base64 */; mimeType:
  string }`. A `UserMessage.content` is `string | (TextContent | ImageContent)[]`
  (~line 384) — i.e. a string is still valid (back-compat), but content is
  fundamentally a **block list**. Tool results are `(TextContent | ImageContent)[]`
  too (~line 407), so a tool can return an image.
- **`read_file` yields an image block** ([`read.ts`](../../../pi/packages/coding-agent/src/core/tools/read.ts)):
  detects MIME by content (`detectImageMimeType`), reads binary, runs
  `processImage`, and returns `{ type: "image", data, mimeType }` alongside a text
  note `Read image file [image/png]`. Tests
  ([`tools.test.ts`](../../../pi/packages/coding-agent/test/tools.test.ts) ~line 188):
  a PNG stored as `image.txt` is detected by **magic** and returned as an
  `image/png` block; a **BMP is converted to PNG** (`[Image converted from image/bmp
  to image/png.]`, first byte `0x89`); a `.png` file whose bytes aren't a PNG comes
  back as **text** (no image block).
- **Resize / convert for model limits** ([`image-process.ts`](../../../pi/packages/coding-agent/src/utils/image-process.ts),
  `image-resize.ts`, `image-convert.ts`): auto-resize to a max (2000×2000 default),
  convert to a supported inline format, and **omit with a note** if it can't be
  reduced below the inline size limit.
- **Capability gate** ([`read.ts`](../../../pi/packages/coding-agent/src/core/tools/read.ts)
  ~line 88): if `model.input` doesn't include `"image"`, the image is dropped with
  `[Current model does not support images. The image will be omitted…]`.
- **Provider encode:** [`anthropic-messages.ts`](../../../pi/packages/ai/src/api/anthropic-messages.ts)
  ~line 143 emits `{type:"image",source:{type:"base64",media_type,data}}` and adds a
  placeholder text block when only images are present;
  [`openai-completions.ts`](../../../pi/packages/ai/src/api/openai-completions.ts)
  ~line 944 emits `{type:"image_url",image_url:{url}}`. Tool-result images are
  covered by `image-tool-result.test.ts`.

**hermes** does vision as a **tool** (`vision_analyze_tool`): download an image
URL, base64-encode it, and route to a vision-capable auxiliary model — plus
binary/PDF detection in `file_operations.py` / `binary_extensions.py` and image-gen
plugins tested under `tests/plugins/image_gen/`. It does not model multimodal
*message* content as a first-class typed block the way pi/opencode do; it's the
"vision via a side tool" shape.

**opencode** attaches files to a user message as **FileParts** and maps them in
[`to-llm-message.ts`](../../../opencode/packages/core/src/session/runner/to-llm-message.ts)
via `media(file) → { type:"media", mediaType, data, filename, metadata }`, appended
after the text part (`content: [{type:"text",…}, ...files.map(media)]`). Its `read`
tool returns image media detected by file magic (see 03-file-read-write §3). Tests:
`tool-read.test.ts`, `session-runner-message.test.ts`.

## Completeness gaps

Behaviour agent-seddon must add/guarantee to be the most complete (spec only — do
**not** implement here):

- **Typed content blocks.** Introduce `ContentBlock` in `agent-core`:
  `Text(String) | Image { media_type: String, data: Bytes /* raw or base64 */ } |
  Document { media_type: String, data: Bytes, name: Option<String> }`. `Message.content`
  becomes `Vec<ContentBlock>`; `Observation.content` likewise gains blocks so a tool
  result can carry an image (a screenshot).
- **Back-compat with `String`.** The `Message::{system,user,assistant,tool}`
  constructors keep their `impl Into<String>` signature (fold a string into a single
  `Text` block); a `content_text()` accessor concatenates `Text` blocks so existing
  text-only call sites are untouched. Serde must round-trip a **bare string** into
  one `Text` block and a **block list** verbatim (custom `Deserialize` / untagged
  enum) so old sessions and configs still load.
- **Proto extension.** Add a `ContentBlock` message + `oneof` (`text | image |
  document`) to [`common.proto`](../../crates/agent-proto/proto/agent/v1/common.proto)
  with `bytes data` + `string media_type`; change `Message.content` to
  `repeated ContentBlock` (keep field 2 semantics back-compat: a legacy `string
  content` maps to one text block). Bump the `buf.image.binpb` baseline via
  `nix run .#buf-image` (additive block list should pass `buf breaking`; the
  `string content` → `repeated` change is the deliberate wire edit to record).
- **Provider encoding.** Anthropic adapter emits
  `{type:"image",source:{type:"base64",media_type,data}}` (and a placeholder text
  block when a turn is image-only); OpenAI-compat adapter emits
  `{type:"image_url",image_url:{url:"data:<mime>;base64,<data>"}}`. Documents map to
  each vendor's document/file block where supported, else degrade to a text note.
- **Image resize / downscale.** Before encoding, if an image exceeds a per-model
  inline byte/dimension cap, downscale/re-encode (e.g. to PNG/JPEG) to fit; if it
  still can't fit, **omit with a note** (mirrors pi's "could not be resized below
  the inline image size limit"). Format conversion (BMP→PNG) where the source isn't
  a supported inline type.
- **Tool results carrying images.** `read_file` of a PNG returns a `Text` note +
  an `Image` block instead of the current "binary file … not shown" drop; the loop
  and providers must forward `Observation` image blocks into the next request.
- **Capability gating.** Add `supports_vision` to `ModelCapabilities`
  (proto ~line 96 already has `supports_tools`/`context_window`). If the selected
  model isn't vision-capable, drop image blocks with an explicit note (never send an
  unsupported block that errors the whole request).
- **Metered by modality (differentiator).** A content-block counter labelled by
  `modality = text|image|document` and `direction = sent|received`; an OTel span
  `multimodal.encode` over the decode/resize path carrying `media_type`,
  `bytes_in`, `bytes_out`, `resized`, `converted` attributes (matching the #44
  span-attribute pattern).

## Table-driven test plan

Tests split across the layers they touch, each mirroring the existing table style
(`#[rstest]` `#[case::…]`, `agent-testkit` doubles). Use a **deterministic tiny
fixture image** — a 1×1 PNG as a `const &[u8]` (the 67-byte minimal PNG) in
`agent-testkit` — so encode/resize cases are byte-reproducible and need no network
or image assets. Case prefixes: `positive_`/`negative_`/`corner_`/`boundary_`;
`(port: <peer>)` / `(new: agent-seddon)` provenance tags.

**A. Content-block model + serde back-compat** — in `crates/agent-core/src/lib.rs`
`mod tests`:

```rust
#[rstest]
#[case::positive_string_folds_to_one_text_block(          // (new: agent-seddon)
    json!("hello"),
    vec![Block::Text("hello".into())])]
#[case::positive_block_list_roundtrips(                   // (port: pi UserMessage.content)
    json!([{"type":"text","text":"hi"},
           {"type":"image","media_type":"image/png","data":TINY_PNG_B64}]),
    vec![Block::Text("hi".into()),
         Block::Image{ media_type:"image/png".into(), /* … */ }])]
#[case::corner_text_accessor_concats_text_blocks(         // (new: agent-seddon)
    json!([{"type":"text","text":"a"},{"type":"text","text":"b"}]),
    /* content_text() == "ab" */ vec![/* … */])]
#[case::boundary_empty_content_list(json!([]), vec![])]   // (new: agent-seddon)
fn message_content_deserialize(#[case] raw: Value, #[case] want: Vec<Block>) { /* … */ }
```

**B. Provider encoding** — in the provider crates' `mod tests` (extend the existing
Anthropic block-builder table around `anthropic.rs` ~line 518, and the OpenAI
converter):

```rust
#[rstest]
#[case::positive_text_block_unchanged(                    // (new: agent-seddon; back-compat)
    Message::user("hi"),
    json!([{"type":"text","text":"hi"}]))]
#[case::positive_image_to_anthropic_source(              // (port: pi anthropic-messages)
    user_with_image("image/png", TINY_PNG),
    json!([{"type":"image","source":{"type":"base64",
            "media_type":"image/png","data":TINY_PNG_B64}}]))]
#[case::corner_image_only_turn_gets_placeholder_text(    // (port: pi)
    user_image_only("image/png", TINY_PNG),
    /* content includes a {"type":"text"} placeholder + the image */ json!(/* … */))]
fn anthropic_encodes_blocks(#[case] msg: Message, #[case] want: Value) { /* … */ }

#[rstest]
#[case::positive_image_to_openai_image_url(              // (port: pi openai-completions)
    user_with_image("image/jpeg", TINY_JPEG),
    json!([{"type":"image_url",
            "image_url":{"url":"data:image/jpeg;base64,…"}}]))]
fn openai_encodes_blocks(#[case] msg: Message, #[case] want: Value) { /* … */ }
```

**C. Resize / convert / capability gate** — in the image-process helper + the
provider gate:

```rust
#[rstest]
#[case::boundary_oversize_image_downscaled(              // (port: pi resizeImage)
    big_png(4000, 4000), /* cap */ 2000,
    Outcome::Resized { max_dim: 2000 })]
#[case::corner_bmp_converted_to_png(                     // (port: pi image-convert BMP→PNG)
    TINY_BMP, Outcome::Converted { from:"image/bmp", to:"image/png" })]
#[case::negative_unresizable_image_omitted_with_note(   // (port: pi "could not be resized")
    undecodable_bytes(), Outcome::OmittedWithNote)]
#[case::negative_non_vision_model_drops_image(          // (port: pi model.input !image)
    Model{ supports_vision:false, .. },
    Outcome::DroppedWithNote { note:"does not support images" })]
fn image_process_and_gate(#[case] input: /* … */, #[case] want: Outcome) { /* … */ }
```

**D. `read_file` → image block** — extend `crates/agent-tools/src/core.rs`
`mod tests` (today `read_file_binary_is_reported_not_dumped` ~line 359 asserts the
drop; add the positive image path):

```rust
#[tokio::test]
async fn read_file_png_yields_image_block() {            // (port: pi/opencode magic-detect)
    // write TINY_PNG bytes to `pic.png`; run ReadFileTool; assert the Observation
    // carries an Image block { media_type:"image/png", data:non-empty } plus a
    // "[image/png]" text note — NOT the "binary file … not shown" message.
}
#[tokio::test]
async fn read_file_png_bytes_that_are_not_png_stay_text() { // (port: pi tools.test.ts)
    // a `.png` file whose bytes are "definitely not a png" ⇒ text/binary path,
    // no Image block (detect by magic, not extension).
}
```

**E. Proto roundtrip of an image block** — extend
[`crates/agent-grpc/tests/roundtrip.rs`](../../crates/agent-grpc/tests/roundtrip.rs):

```rust
#[rstest]
#[case::positive_image_block_survives_grpc(              // (new: agent-seddon differentiator)
    Message { role: Role::User,
              content: vec![Block::Image{ media_type:"image/png".into(),
                                          data: TINY_PNG.into() }], .. })]
#[case::positive_legacy_string_content_maps_to_text(     // (new: agent-seddon back-compat)
    /* a Message with only text ⇒ decodes to one Text block over TCP + UDS */ )]
fn message_blocks_roundtrip(#[case] msg: Message) {
    // encode → proto → decode over both TCP and UDS; assert media_type + bytes
    // are byte-identical (reflection descriptor still lists Message unchanged
    // except the new ContentBlock oneof).
}
```

## References

- **agent-seddon:**
  [`crates/agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs)
  (`Message`, `Role`, `Observation`),
  [`crates/agent-proto/proto/agent/v1/common.proto`](../../crates/agent-proto/proto/agent/v1/common.proto)
  (`Message`, `JsonValue`, `ModelCapabilities`),
  [`crates/agent-providers/src/anthropic.rs`](../../crates/agent-providers/src/anthropic.rs)
  (content-block builder ~line 347),
  [`crates/agent-providers/src/openai_compat.rs`](../../crates/agent-providers/src/openai_compat.rs),
  [`crates/agent-tools/src/core.rs`](../../crates/agent-tools/src/core.rs)
  (`ReadFileTool` binary path ~line 141),
  [`crates/agent-grpc/tests/roundtrip.rs`](../../crates/agent-grpc/tests/roundtrip.rs),
  [`crates/agent-testkit/src/bench.rs`](../../crates/agent-testkit/src/bench.rs)
  (fixtures),
  and the follow-up note in [`03-file-read-write.md`](03-file-read-write.md).
- **pi:** `pi/packages/ai/src/types.ts` (`ImageContent`, `UserMessage.content`),
  `pi/packages/coding-agent/src/core/tools/read.ts`,
  `pi/packages/coding-agent/src/utils/image-process.ts` (+ `image-resize.ts`,
  `image-convert.ts`),
  `pi/packages/ai/src/api/anthropic-messages.ts`,
  `pi/packages/ai/src/api/openai-completions.ts`,
  `pi/packages/coding-agent/test/tools.test.ts`,
  `pi/packages/ai/test/image-tool-result.test.ts`,
  `pi/packages/ai/test/openrouter-images.test.ts`.
- **hermes:** `hermes-agent/tools/vision_tools.py`,
  `hermes-agent/tools/image_source.py`, `hermes-agent/tools/file_operations.py`,
  `hermes-agent/tools/binary_extensions.py`,
  `hermes-agent/tests/plugins/image_gen/`.
- **opencode:** `opencode/packages/core/src/session/runner/to-llm-message.ts`
  (`media()` FilePart mapping), `opencode/packages/core/src/tool/read.ts`,
  `opencode/packages/core/test/tool-read.test.ts`,
  `opencode/packages/core/test/session-runner-message.test.ts`.

## Harness obligations

- **Seam/schema:** no new seam trait — this is a **schema change** to `agent-core`
  (`Message`/`Observation` gain a `ContentBlock` list, with a `content_text()`
  accessor and string-folding constructors for back-compat) rippling into the
  `LlmProvider` adapters and the `read_file` `Tool`.
- **Proto + gRPC:** extend `common.proto` (`ContentBlock` oneof `text|image|
  document`, `Message.content` → `repeated`, `ModelCapabilities.supports_vision`);
  the gRPC **roundtrip test is extended** for image blocks (TCP + UDS); reflection
  descriptors change only by the additive `ContentBlock`, otherwise unchanged.
  Commit the `buf.image.binpb` bump (`nix run .#buf-image`) — the `string content`
  → `repeated ContentBlock` edit is the deliberate wire change to record.
- **Metrics + OTel:** content-block **counter by modality**
  (`text|image|document` × `sent|received`) in `agent-metrics`, wired through a
  `metered.rs` decorator, plus a `multimodal.encode` span with
  `media_type`/`bytes_in`/`bytes_out`/`resized`/`converted` attributes.
- **Bench:** iai-callgrind bench for the **CPU hot path = image decode/resize** over
  the deterministic tiny-PNG (and an oversize synthetic) fixture, with an Ir ceiling
  in `nix/checks/bench.nix` (image resize is genuine, bounded CPU work — unlike the
  I/O-bound file tools that document a bench skip).
- **Leak:** a dhat `tests/leak.rs` case over the **block encode path** (decode →
  resize → base64 → provider JSON) asserting the hot path frees its buffers and
  stays under an allocation budget.
