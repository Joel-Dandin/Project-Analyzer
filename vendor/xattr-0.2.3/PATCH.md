# xattr 0.2.3 compatibility patch

Source: [xattr 0.2.3 on crates.io](https://crates.io/crates/xattr/0.2.3), by Steven Allen and contributors, under MIT OR Apache-2.0. Both upstream license texts are included. `Cargo.toml` is the upstream original manifest (`Cargo.toml.orig` in the crate archive).

`gpui` 0.2.2 pulls in this version through `gpui_http_client` and `zed-async-tar`. The only source change is in `src/sys/mod.rs`: use `libc::ENODATA` on Linux and Android now that current `libc` no longer exports the `ENOATTR` alias there. macOS and BSD still use their native `ENOATTR`. Remove this patch once the locked GUI dependency graph no longer needs `xattr` 0.2.x.
