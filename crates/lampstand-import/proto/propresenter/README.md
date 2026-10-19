# ProPresenter 7 protobuf definitions

Unofficial, reverse-engineered `.proto` files from
<https://github.com/greyshirtguy/ProPresenter7-Proto> (directory `proto/`,
commit 1c019ab1897286c9bc611f0a10f9f7da8d01dcbb), copyright (c) 2023
greyshirtguy, MIT licence (see `LICENSE`, which is GPL-3.0 compatible).

Only the files reachable from `presentation.proto` are vendored; they are
unmodified. They are not made or endorsed by Renewed Vision. `build.rs`
compiles them with `protox` (pure Rust, no `protoc`) and `prost-build`.
