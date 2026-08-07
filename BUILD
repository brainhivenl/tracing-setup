load("@crates//:defs.bzl", "all_crate_deps")
load("@rules_rust//rust:defs.bzl", "rust_library", "rust_test")

rust_library(
    name = "tracing-setup",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = all_crate_deps(normal = True),
    visibility = ["//visibility:public"],
)

rust_test(
    name = "tracing-setup_test",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = all_crate_deps(normal = True),
)
