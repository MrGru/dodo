//! Persistence seams for Cleaner. Currently one: the orphan-detection "keep"
//! list. A sibling of `core`, `macos`, `state` and `views` — like
//! `api_explorer::services`, `updater::services` and `quick_nav::services`,
//! this is where a store's trait and disk implementation live, kept apart
//! from the plain domain data it persists (`core::ignore`) and from the view
//! code that triggers a save (`views::cleaner_view`).

pub mod ignore_store;
