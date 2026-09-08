# GPUI Kit 0.6 migration

Dodo depends directly only on the `gpui-kit` 0.6 facade (`default-features = false`, `component`). Styled-component imports use `gpui_kit::component`, whose enabled `component` feature re-exports the matching `gpui-component` 0.6 crate. The application bootstraps its own `Assets` with `gpui_kit::application().with_assets(Assets)` and calls `gpui_kit::init(cx)` before Dodo initialization.

## Usage inventory and migration matrix

| API category | Dodo paths | 0.6 result |
|---|---|---|
| Single-line input | `src/settings`, `src/quick_nav`, Docker list searches, Database forms, API key/value rows, Flow prompts | `InputState` / `Input` retained |
| Multiline text | `dodo-encoder-decoder`, API Explorer bulk key/value editing, Flow inline labels | `TextareaState` / `Textarea` |
| Source editors | JSON Formatter; Docker inspect; Database SQL/query forms; API request/response/scripts; Mermaid; Encoder/Decoder JWT | `EditorState` / `Editor`, preserving language, wrapping, read-only and change behavior |
| Interactive tables | Cleaner, Database result/object/query views, Quick Navigation | Already `DataTable`; no legacy `Table` or `row_selector` call sites |
| Separators, histories, docking, WebView | workspace-wide audit | No `Divider`, `DescriptionList::divider`, upstream `History`, docking, or WebView integration. `dodo-flow` command history remains Dodo domain state. |
| Smaller renamed APIs | workspace-wide audit | No `is_eof`, `can_go_to_definition`, `can_zoom`, `can_close`, or `ComboboxTriggerCtx` callers. |

The v0.6 scroll API removes the old `ScrollbarShow` builder; Flow keeps its vertical scrollbar with the supported base `Scrollbar::vertical` renderer. Mermaid's preview now supplies both destination and image bounds to GPUI's changed `paint_image` call.

## Assets and dependency checks

`src/assets.rs` is Dodo's only application asset source. The former complete `gpui-component-assets` fallback was removed. Thirty shared SVGs used by Dodo's existing `AppIcon` variants are individually embedded under `assets/icons`; Dodo branding, tool, tray, language, Docker, database, and application-icon paths are unchanged. `gpui-component` remains transitively in `Cargo.lock` through `gpui-kit`'s enabled `component` feature, and it has a non-optional dependency on `gpui-kit-assets`. Dodo does not select the facade `assets` feature or use `gpui_kit::assets::Assets`.

## Upstream references

- [GPUI Kit 0.6.0 release notes](https://github.com/longbridge/gpui-kit/releases/tag/v0.6.0)
- [Installation](https://gpui-kit.com/docs/installation/)
- [Assets](https://gpui-kit.com/docs/assets/)
