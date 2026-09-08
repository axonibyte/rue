#!/usr/bin/env bash
# ci/build-target.sh  build rue's release binaries for one target triple.
# Usage: bash ci/build-target.sh <target-triple>
# All per-target knowledge (linkers, toolchains, std availability) lives here;
# bitbucket-pipelines.yml just dispatches. Per-OS knowledge lives in exactly
# three places in this repository (ROADMAP.md section 4.5); this is one.
set -euo pipefail

TARGET="${1:?usage: build-target.sh <target-triple>}"

export CARGO_HOME="${CARGO_HOME:-$BITBUCKET_CLONE_DIR/.cargo_cache}"

# Pinned: -Z build-std against a floating nightly breaks spontaneously and
# makes tag builds unreproducible. Bump deliberately, together with the image
# pin in bitbucket-pipelines.yml and rust-version in Cargo.toml.
NIGHTLY="nightly-2026-08-01"

# Every binary the workspace ships, on every target (D-031: Windows is not a
# client-only build; macOS is cross-built the same way). A name that does not
# exist yet is skipped (rued arrives in Phase 3's daemon unit, rue-hook in
# Phase 4); none existing is a failure.
BINS="rue rued rue-hook"

apt_install() {
    apt-get update
    apt-get install -y --no-install-recommends "$@"
}

# Zig ships FreeBSD libc headers, letting cargo-zigbuild cross-link FreeBSD
# binaries from Linux with no docker and no sysroot images.
install_zigbuild() {
    apt_install python3-pip
    pip3 install --break-system-packages cargo-zigbuild
}

# Phase 1 acceptance (ROADMAP.md): clippy clean on every target, with the
# toolchain that builds it. Clippy needs the target's std, not its linker,
# so it runs on this host for every target that has a rustup std.
lint() {
    cargo clippy --workspace --all-targets --target "$TARGET" --locked -- -D warnings
}

build() { # tries offline first, falls back to online
    cargo build --workspace --target "$TARGET" --release --locked --offline ||
    cargo build --workspace --target "$TARGET" --release --locked
}

rustup component add clippy

case "$TARGET" in
    x86_64-unknown-linux-gnu)
        lint
        build
        ;;

    aarch64-unknown-linux-gnu)
        # libc6-dev-arm64-cross is only a Recommends of the gcc package, so
        # with --no-install-recommends it must be named explicitly; without it
        # the cross-gcc has no target libc headers/CRT.
        apt_install gcc-aarch64-linux-gnu libc6-dev-arm64-cross
        rustup target add "$TARGET"
        export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc
        lint
        build
        ;;

    x86_64-unknown-freebsd)
        # Tier 2: prebuilt std exists; stable toolchain + zig linker.
        install_zigbuild
        rustup target add "$TARGET"
        lint
        cargo zigbuild --workspace --target "$TARGET" --release --locked
        ;;

    aarch64-unknown-freebsd)
        # Tier 3: no prebuilt std, so compile it with nightly -Z build-std.
        install_zigbuild
        rustup toolchain install "$NIGHTLY" --profile minimal --component rust-src --component clippy
        cargo "+$NIGHTLY" clippy --workspace --all-targets --target "$TARGET" --locked -Z build-std=std,panic_abort -- -D warnings
        cargo "+$NIGHTLY" zigbuild --workspace --target "$TARGET" --release --locked -Z build-std=std,panic_abort
        ;;

    x86_64-pc-windows-gnu)
        # Statically linked C runtime via .cargo/config.toml: the exe carries
        # no mingw DLL dependency, under wine or on a Windows machine.
        apt_install gcc-mingw-w64-x86-64
        rustup target add "$TARGET"
        export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc
        lint
        build
        ;;

    x86_64-apple-darwin|aarch64-apple-darwin)
        # Tier 1 targets with prebuilt std; zig links Mach-O against its own
        # bundled libSystem, so no macOS SDK is involved (ROADMAP.md section
        # 12): the workspace links libSystem alone, and
        # tools/lint-darwin-deps.sh keeps every framework-linking crate out.
        # Built and packaged here; executed and signed nowhere until a Mac
        # exists (section 11).
        install_zigbuild
        rustup target add "$TARGET"
        lint
        cargo zigbuild --workspace --target "$TARGET" --release --locked
        ;;

    *)
        echo "unknown target: $TARGET" >&2
        exit 1
        ;;
esac

# --- package ---------------------------------------------------------------
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)
if [[ -z "$VERSION" ]]; then
    echo "no workspace version in Cargo.toml" >&2
    exit 1
fi
mkdir -p dist
packaged=0
for bin in $BINS; do
    if [[ "$TARGET" == *windows* ]]; then
        src="target/$TARGET/release/${bin}.exe"
        dst="dist/${bin}-v${VERSION}-${TARGET}.exe"
    else
        src="target/$TARGET/release/${bin}"
        dst="dist/${bin}-v${VERSION}-${TARGET}"
    fi
    if [[ -f "$src" ]]; then
        cp "$src" "$dst"
        echo "packaged $dst"
        packaged=$((packaged + 1))
    fi
done
if [[ "$packaged" -eq 0 ]]; then
    echo "no binaries produced for $TARGET (looked for: $BINS)" >&2
    exit 1
fi
