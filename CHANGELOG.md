# Changelog

## v0.2.2

- Add lazy `Send` streams of owned repository, issue and pull-request pages.
- Share pagination between streams, collections and callbacks, preserving validation, sorted final lists, bounded concurrency and cancellation.

## v0.2.1

- Document `async move` capture and the boxed-future workaround for generic `Send` progress adapters ([#3](https://github.com/davlgd/github-rust/issues/3)).
- Add a compiled Tokio example and regression checks for repository, issue and PR callbacks in `Send` futures and multithreaded tasks.

## v0.2.0

- Add authenticated viewer and organization discovery, owned repository inventories, and open issue/PR collections.
- Support asynchronous page callbacks, cancellation and bounded repository concurrency with strict pagination checks.
- Preserve issue/PR authors, labels, assignees, comments, draft status and review decisions; reject truncated embedded metadata.
- Document the new collection APIs and add an account overview example.
- Separate public repository models from API responses, preserve opaque node IDs and distinguish unknown counts from zero.
- Correct watcher and open-issue counts, expose language completeness, and deprecate node-ID decoding.
- Add an explicit client builder, configurable fallback policy, structured errors and per-resource quota metadata.
- Harden search input validation, request sizing and pagination; replace placeholder checks with HTTP mock tests and CI across stable Rust and MSRV.
- Update dependencies while retaining Rust edition 2024 and MSRV 1.92.

## v0.1.0

Initial public release
