---
name: dodo-api-explorer-internals
description: Deep internals of src/api_explorer/ that no single file makes obvious - the send pipeline's exact step order and why pre-request scripts run before variable substitution but prepare runs last, the QuickJS sandbox and its Eval-intrinsic gotcha, script-consent gating and provenance, why pm.test/pm.expect are JavaScript not Rust bindings, the syntax-checker and Format action, why codegen and curl-parsing are separate modules, the secret-variable masking policy for generated code, curl paste-to-rebuild, tab-title derivation, and the min_w_0 layout rule. Load before touching anything under src/api_explorer/ - the send pipeline, scripting/sandbox, consent gating, codegen/curl, collections, or tab/column layout.
---

`src/api_explorer/` is the largest module in dodo and the only one with thirteen things worth
knowing before touching it that no single file makes obvious:

- **The whole send pipeline lives in `services/send.rs`, in this order:**
  `pre-request script → resolve {{name}} → prepare → Transport::execute → post-response script`.
  It is one blocking function over trait objects, which is what makes the *ordering* unit-testable
  with a fake transport and a fake engine and no `Window`. Its module doc records why the script
  runs **before** substitution (the scripting plan said the opposite and was wrong: the shipped
  `pm.variables.set("timestamp", …)` template promises the value resolves in that same request)
  and why `prepare` nonetheless stays **last** (a header or URL a script wrote still goes through
  dodo's own validation). `models/interpolate.rs` owns substitution itself — the escape rule
  (`\{{`), the recursion guard and the decision that an unresolved reference *fails* the request
  rather than being sent literally or blanked; it is pure and exhaustively table-tested.
- **The two hooks fail in opposite directions, deliberately.** A failed *pre-request* script stops
  the send (a half-configured request produces a response nobody can reason about). A failed
  *post-response* script **must not lose the response** — the request already happened, and the
  response is the evidence the user needs to fix the script — so it becomes a Console line plus the
  Tests tab's error banner while `SendOutcome::result` stays `Ok`. Both are argued in
  `services/send.rs`'s module doc and both are tested.
- **Test results attach to `ResponseState`, never to `Exchange`** (`models/test_result.rs`).
  `Exchange` is protocol-neutral and must not learn about scripting; and a *pre-request* script can
  define tests for a request that then never got a response, which `Exchange` could not hold at
  all. History keeps only a `TestSummary` — it is capped by count, not bytes, and a row has space
  for one badge. `Failed` and `Errored` stay distinct because they tell the user whether their API
  or their script is wrong.
- **Sending is one background job — script, `prepare` and the request together.** `prepare` used
  to run on the UI thread; it does not any more, because encoding a body may read a file (a
  multipart file part, a binary body), and a script may loop for its whole 2 s budget.
  `state/tab.rs::send` spawns the lot, so a validation error lands one task hop after the click.
  **No file is ever read on the UI thread**: `services/file_picker.rs` runs the picker and its
  `stat` off-thread, and `services/http/upload.rs` is the only place bytes are read — reached
  solely from `prepare`, with a stated size cap. Do not reintroduce a synchronous `prepare` call
  from a view.
- **`services/script/` is the only module that may name `rquickjs`**, and `quickjs.rs`'s doc is
  the authority on the sandbox: the positive intrinsic allowlist, the 2 s deadline / 16 MiB / 256
  KiB caps, one fresh runtime per run, and why `pm.sendRequest` is denied by *not existing* while
  still failing with a message that names it. Read it before changing anything about what a script
  can reach. One correction it records, because it cost a build to find: **the `Eval` intrinsic
  cannot be left out.** `JS_AddIntrinsicEval` registers no global — it sets `ctx->eval_internal`,
  which `JS_EvalInternal` refuses to run without — so omitting it breaks dodo's own `ctx.eval`,
  not just the script's. `eval` confers no capability the script lacks; what it costs is
  legibility, and any future static API scan has to say so.
- **A script's provenance is a property of the request, not of the text.**
  `RequestSnapshot::script_origin` is set to `Imported` by `services/collection_import.rs` and
  never changed by editing, so an imported script cannot be laundered past the consent gate.
  Editing changes the *content hash*, which re-arms the gate — the two halves of a
  `models/script_consent.rs::ConsentKey`. **One key covers both hooks** (they are hashed together):
  an approval given when only the pre-request hook ran said nothing about a post-response script,
  so honouring it for one would be exactly the laundering the gate exists to stop. That module owns
  the whole policy; approvals persist in a third `data_dir()` file.
- **`pm.test` and `pm.expect` are written in JavaScript, not in Rust bindings** — `PRELUDE` in
  `services/script/quickjs.rs`, handed its two host functions as *arguments* so no `__`-prefixed
  global exists for a script to find. Chai's chain words are self-references and `.not` is a getter
  (a plain property would recurse forever). The matcher set is exactly `report.md` §3.2's and no
  more: an unsupported matcher must fail as a missing function rather than quietly pass.
- **The script editors' syntax check uses the engine that will run the script**
  (`ScriptEngine::check` → `Module::declare`, QuickJS's compile-only flag), debounced in
  `state/tab.rs`. That is the whole point: a second parser could underline code the engine then
  runs happily. Module goal accepts a little more than script goal (top-level `await`, `import`),
  which makes the residual disagreements *false negatives* — the safe direction. The **Format**
  action beside it is deliberately *not* a JavaScript formatter: `models/script_format.rs` only
  re-indents and normalises blank lines, and its doc records why (a real one, `dprint-plugin-
  typescript`, measured at +2.8 MB — 12.7% — on a binary that had already grown for the engine).
  Lines inside a template literal or block comment are emitted byte-for-byte.
- **Code generation and cURL parsing are separate modules on purpose.**
  `services/curl.rs` reads one language; `services/codegen/` writes four (cURL, `fetch`, `axios`,
  `XMLHttpRequest`). They share no code and are shaped nothing alike — a tokenizer plus an option
  table against four pure emitters over a single normalized form. **`services/codegen/normalize.rs`
  is the piece to understand first**: it flattens a `RequestSnapshot` into method / one absolute URL
  / one header list with auth folded in / one body, following `prepare`'s exact deference order and
  reusing `auth::apply`, `effective_pairs` and `request_body::form_escape` rather than re-deriving
  them. Four ad-hoc walks would have given "does the API key ride in the query" four answers. It
  validates nothing and reads no file — both are stated there with why. The two directions are
  joined by a **round-trip property test** in `services/codegen/curl.rs`: generate, hand it to
  `curl::parse`, and require both to normalize to the same `NormalizedRequest`. The equivalence is
  over the *wire request*, not the snapshot (an Auth-tab entry comes back as a header), and the three
  cases where even that cannot hold are named in that module's doc.
- **Generated code withholds a `secret` variable and says so; it withholds nothing else.** A
  reference to a variable marked `secret` is emitted as the literal `{{name}}`, via
  `VariableSet::with_secrets_masked` — which sets the masked value to `\{{name}}` so the substituter's
  *existing* escape rule does the work, covering nesting and making recursion impossible. A token or
  password typed straight into the Auth tab **is** in the copied text, because it has no name to
  stand in for it; the dialog's notice is never absent and says exactly that, and a
  **Resolve secret variables** toggle resolves them behind a danger-coloured warning. The whole
  policy and its reasoning live in `services/codegen/mod.rs`'s module doc — read it before changing
  what reaches the clipboard. What JavaScript cannot express (a form-data FILE row, a binary body)
  becomes an **undeclared identifier** with a comment naming the path, so running the snippet
  unchanged throws rather than silently sending an incomplete request; `services/codegen/
  javascript.rs` argues that against the two alternatives.
- **Pasting a cURL command into the URL box rebuilds the whole request** (`services/curl.rs`,
  which is pure and heavily table-tested). Two guards keep it from firing while somebody types the
  word "curl": `state::request::is_bulk_change` (a paste is not a keystroke) and a parse that must
  yield a URL. A parsed command **opens a new tab and restores the field it was pasted into**,
  reusing the current tab only when it is untouched — so it can never overwrite unsaved work.
- **A tab's auto-derived title is the URL's *path*** (`models/tab_title.rs`, `/api/123`, not the
  host), with the host as the fallback for an empty path and the raw text for anything that does
  not parse. An explicitly named tab still wins. The rules live in that module's doc and its tests;
  `state::request::display_name` only chooses between it and the "Untitled" wording.
- **The request and response columns need `min_w_0`, and it is load-bearing.** A flex item
  defaults to `min-width: auto`, so without it the *widest child of the widest tab* sets the
  column's width and everything else — the Send button first — is pushed off the right of the
  window. It is invisible until some pane grows: the Scripts tab's sandbox notice made it show up
  at 1280px. `render_request_editor` and `render_response_viewer` carry the `min_w_0` with the
  reason inline; put one on any new pane rather than trimming the text that exposed it.

`api_explorer` is also the only tool that registers a key binding (`api_explorer::init`, called
from `main` after `gpui_component::init`, same ordering rule as `settings::init`).
