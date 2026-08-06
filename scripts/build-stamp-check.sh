#!/usr/bin/env bash
# #613 acceptance check for crates/freshell-server/build.rs's commit
# stamp: proves a same-branch ref update performed while the ref is
# PACKED (loose file absent at stamp time, written loose later by
# git update-ref — the fetch/ff-pull shape) still restamps
# FRESHELL_BUILD_COMMIT. Step (e) FAILS against the exists()-gated watch
# and PASSES after the unconditional ref watch.
set -euo pipefail

# Isolate from the ambient environment: the assertions below run
# ./target/debug/stampcheck, so a user-level CARGO_TARGET_DIR would
# redirect the build elsewhere and break them; and the throwaway repo's
# commits must not pick up global/system git config (commit.gpgsign=true
# would prompt or fail).
unset CARGO_TARGET_DIR
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORK="$(mktemp -d)"
trap 'rm -rf "${WORK}"' EXIT

echo "--- setup: throwaway git repo + tiny crate embedding build.rs ---"
mkdir -p "${WORK}/proj/src"
cp "${REPO_ROOT}/crates/freshell-server/build.rs" "${WORK}/proj/build.rs"
cat > "${WORK}/proj/Cargo.toml" <<'EOF'
[package]
name = "stampcheck"
version = "0.0.0"
edition = "2021"

[workspace]
EOF
cat > "${WORK}/proj/src/main.rs" <<'EOF'
fn main() {
    println!("{}", option_env!("FRESHELL_BUILD_COMMIT").unwrap_or("unknown"));
}
EOF
cd "${WORK}/proj"
git init -q
git config user.email "check@example.com"
git config user.name "Stamp Check"
git add -A
git commit -qm "c1"
C1="$(git rev-parse HEAD)"
# Settle the index: a same-second add/commit leaves entries racily clean,
# so the `git status` build.rs runs at stamp time would rewrite the index
# DURING the baseline builds — and that index-watch rerun would restamp at
# step (e) for a reason unrelated to the ref gap, masking it. One refresh
# after a second boundary records definitive stat info; no later
# `git status` rewrites the index, so only the ref-watch behavior decides.
sleep 1
git status --porcelain > /dev/null

echo "--- (a)(b) build once, then pack refs (loose ref file goes away) ---"
cargo build -q
git pack-refs --all
if compgen -G ".git/refs/heads/*" > /dev/null; then
  echo "FAIL: loose ref file still present after git pack-refs"; exit 1
fi

echo "--- (c) build in the packed-ref state (stamps under packed refs) ---"
cargo build -q
OUT1="$(./target/debug/stampcheck)"
[ "${OUT1}" = "${C1}" ] || { echo "FAIL: baseline stamp ${OUT1} != ${C1}"; exit 1; }

echo "--- (d) advance the branch ref WITHOUT touching HEAD bytes or index ---"
git commit -q --allow-empty -m "c2"
C2="$(git rev-parse HEAD)"
# The empty commit wrote the ref LOOSE again and did not change the index
# or working tree — exactly the watched-file blind spot.

echo "--- (e) rebuild and assert the NEW sha is compiled in ---"
cargo build -q
OUT2="$(./target/debug/stampcheck)"
if [ "${OUT2}" = "${C2}" ]; then
  echo "PASS: stamp followed the ref update (${C2})"
else
  echo "FAIL: stale stamp ${OUT2}; expected ${C2}"
  exit 1
fi
