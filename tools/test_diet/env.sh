# Source from the worktree root: sets the cargo-llvm-cov instrumentation env
# for plain `cargo test` and clears every QUANTICK_* variable a test could read.
for v in $(env | grep -o '^QUANTICK_[A-Z0-9_]*'); do unset "$v"; done
export CARGO_BUILD_JOBS=2
eval "$(cargo llvm-cov show-env --sh 2>/dev/null)"
# Each run names its own profile files; tools/test_diet/coverage.py sets this.
unset LLVM_PROFILE_FILE
PACKAGES="-p quantick-app -p quantick-control-handlers -p quantick-drawings -p quantick-paper -p quantick-sim -p quantick-chart -p quantick-orderflow -p quantick-sources -p quantick-session -p quantick-workspace -p quantick-engine -p quantick-ui-model"
