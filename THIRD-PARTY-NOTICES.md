# Third-party notices

dodo's own source is licensed under the MIT License — see [`LICENSE`](LICENSE).

dodo is a Rust binary and is statically linked. A build of it therefore contains
compiled code from a large number of third-party crates, under their own
licences. This file records what those are, with emphasis on the components
whose terms differ from dodo's own.

**This file is a factual record, not legal advice.** It states what is linked
and under which licence, as reported by the crates themselves. It does not
interpret those licences and does not tell anyone what they may do.

---

## Open question: distributing built binaries

**Unresolved. Deliberately left unresolved here.**

dodo's source is MIT. Its dependency graph contains crates licensed
**GPL-3.0-or-later** (listed below), and they are reached through `gpui`, which
is the UI framework dodo is built on — they are structural in Zed's crate graph,
not an optional feature, so they are compiled into every dodo binary.

The terms under which a **built dodo binary** may be distributed, given that
combination, have not been decided. MIT-licensing dodo's own source does not by
itself settle the question: what dodo's source is licensed as, and what a linked
binary containing GPL-3.0-or-later object code may be distributed as, are
separate questions.

Nothing in this repository should be read as a decision on that point. It is
recorded here so it is visible rather than assumed, and so that anyone
redistributing a build knows to answer it first.

---

## The GPL-3.0-or-later components

This chain was verified against the current `Cargo.lock` with
`cargo tree -i` and `cargo metadata` (see "Reproducing this file" below):

```
dodo
└── gpui                 Apache-2.0
    └── sum_tree         Apache-2.0
        ├── ztracing     GPL-3.0-or-later
        │   └── zlog     GPL-3.0-or-later
        └── (ztracing also pulls ztracing_macro, GPL-3.0-or-later)
```

| Crate | Version | Licence | Source | Reaches dodo through |
|---|---|---|---|---|
| `ztracing` | 0.1.0 | GPL-3.0-or-later | zed-industries/zed | `gpui` → `sum_tree` |
| `zlog` | 0.1.0 | GPL-3.0-or-later | zed-industries/zed | `ztracing` |
| `ztracing_macro` | 0.1.0 | GPL-3.0-or-later | zed-industries/zed | `ztracing` |

Facts about this chain, stated without inference:

- `gpui` is a direct dependency of dodo, and also a dependency of
  `gpui-component`, `gpui-component-assets` and `gpui_platform`. Every path into
  dodo goes through it, so there is no dodo build that omits it.
- `ztracing` and `zlog` are ordinary library crates. Their compiled code is
  linked into the dodo executable.
- `ztracing_macro` is a procedural macro. It runs in the compiler at build time;
  what ends up in the binary is the code its expansion produces, inside
  `ztracing`.
- Removing them from the graph is not possible from this repository. They are
  not behind a cargo feature dodo controls; dropping them would mean not
  depending on `gpui`.

---

## The rest of the graph

Over the locked graph as resolved for a macOS arm64 host with `--all-features`,
following normal (non-dev, non-build) dependency edges — 461 distinct packages
including dodo itself:

| Licence (as declared) | Packages |
|---|---|
| MIT / Apache-2.0 dual, in its various spellings | 265 |
| MIT only | 93 |
| Apache-2.0 only | 22 |
| Unicode-3.0 (and one `(MIT OR Apache-2.0) AND Unicode-3.0`) | 19 |
| Other permissive: BSD-2/3-Clause, ISC, Zlib, 0BSD, Unlicense, BSL-1.0, CC0-1.0, Apache-2.0 WITH LLVM-exception, in dual or combined expressions | 54 |
| **GPL-3.0-or-later** | **3** |
| MPL-2.0 | 2 |
| `bzip2-1.0.6` | 1 |
| No `license` field declared | 2 |

Individually notable, beyond the GPL crates above:

| Crate | Licence | Note |
|---|---|---|
| `nucleo-matcher` | MPL-2.0 | dodo's fuzzy matcher; a direct dependency. |
| `option-ext` | MPL-2.0 | Transitive, via `dirs`. |
| `libbz2-rs-sys` | bzip2-1.0.6 | A permissive BSD-style licence, transitive via `bzip2`. |
| `aws-lc-sys` | `ISC AND (Apache-2.0 OR ISC) AND Apache-2.0 AND MIT AND BSD-3-Clause AND …` | The rustls crypto provider's C sources; the expression is a conjunction because the vendored sources are multi-origin. |
| `gpui_shared_string`, `gpui_util` | not declared | Both from zed-industries/zed, which as a repository is Apache-2.0/GPL-3.0/AGPL-3.0 mixed per crate. Neither crate carries a `license` field in its manifest, so no licence can be reported for them from the metadata alone. |

The full per-crate list is not reproduced here; it is derivable exactly (see
below) and would go stale the moment `Cargo.lock` moves.

---

## Reproducing this file

Everything above comes from the checked-in `Cargo.lock`:

```sh
# per-crate licences, whole graph
cargo metadata --locked --format-version 1 --all-features

# licences along real (non-dev, non-build) edges only
cargo tree --locked --all-features -e normal --prefix none --format '{p}|{l}'

# why a particular crate is in the graph
cargo tree --locked -i ztracing --edges normal

# the policy check, including licences
cargo deny --all-features check
```

`deny.toml` encodes the policy: permissive licences are allowed, and the three
GPL-3.0-or-later crates are **not** added to its allow list or exceptions, so
`cargo deny check licenses` reports them every time it runs. That is intended —
see the header of `deny.toml`. Run against this commit with cargo-deny 0.20.2,
`check licenses` rejects those three crates and no others.

Counts in this file are for the lock file as of the commit that last touched
this document. Re-run the commands above rather than trusting the numbers.
