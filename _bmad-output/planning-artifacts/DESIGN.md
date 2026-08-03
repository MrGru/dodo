# Visual Design Spine — dodo

**Source:** `CLAUDE.md`, `src/layout.rs`, `gpui-component-recipes` and `dodo-theming-settings` skills
**Stage:** Phase 2/3 — visual spine (layout, theming)

## Layout framework

A single centered 900x620 window: collapsible sidebar + main pane, built entirely from
`gpui-component` widgets (no custom widget toolkit).

```
┌──────────────────────────────────────────────────────────────────┐
│  Sidebar (collapsible, icon-only when collapsed)  │  Main pane   │
│  ─ Json formatter                                 │  (View enum  │
│  ─ Encoder / Decoder                              │   contents)  │
│  ─ API Explorer                                   │              │
│  ─ Docker                                          │              │
│  ─ Database Explorer                               │              │
│  ─ (footer) Settings · Check for updates           │              │
└──────────────────────────────────────────────────────────────────┘
```

- The sidebar is **flat** — every tool, Docker included, is one top-level `SidebarMenuItem` with
  no children, because an icon-collapsed sidebar renders no children at all (`docker/mod.rs`).
  Docker's four pages instead live on their own vertical rail inside the Docker view.
- Every request/response column inside API Explorer carries `min_w_0` — a flex item defaults to
  `min-width: auto`, so without it the widest child of the widest tab sets the column's width and
  pushes the Send button off-window. This was invisible until the Scripts tab's sandbox notice
  made it show up at 1280px; put one on any new pane rather than trimming the text that exposed it.

## Theming

- Theme JSON is vendored and reaches `ThemeRegistry`; writing `gpui_component::Theme` applies a
  theme change **live**, with no restart — see the `dodo-theming-settings` skill before touching
  any of this.
- Font size, border radius, and language are the other live-applying settings; none of them persist
  across restarts except through the five explicit `data_dir()` files (see
  `implementation-artifacts/env-vars-and-config.md`'s neighbor doc on persistence).

## Widgets introduced per tool (visual only)

| Widget | Where | Notes |
|---|---|---|
| Inline diagnostic | JSON Formatter | Parse error shown inline, not as a separate panel |
| JWT split view | Encoder/Decoder | Header/payload/signature as three panes |
| Request/response tab strips | API Explorer | Params/Headers/Body/Auth/Scripts on the request side; Headers/Cookies/Tests/Console/Body on the response side |
| Script consent dialog | API Explorer | `window.open_dialog`, not a page-level scrim |
| DataTable | Database Explorer | One height knob shared by header and body rows; cells are `overflow_hidden` with the size's own vertical padding removed |
| Disclosure chevron | Database Explorer tree | Dodo draws its own — `gpui_component`'s tree widget draws none |
| Detail dialog | Docker | A `window.open_dialog` entity (not page-tree scrim) so it repaints on its own `cx.notify()` and can `.occlude()` clicks underneath it |

## Components that must not change casually (regression risk)

- **Sidebar flatness** — reintroducing a nested group for any tool breaks that tool when the
  sidebar is icon-collapsed.
- **`min_w_0` on API Explorer's request/response columns** — removing it silently reintroduces the
  Send-button-pushed-off-window bug the first time any pane's content grows.
- **The Database Explorer's disclosure chevron** — a node whose children haven't been fetched must
  still get a placeholder child, or it draws no expand affordance and can never be opened
  (`TreeItem::is_folder` is `children.len() > 0`).
