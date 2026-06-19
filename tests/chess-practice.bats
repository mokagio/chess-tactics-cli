#!/usr/bin/env bats
# shellcheck disable=SC2030,SC2031

setup() {
  wrapper="$BATS_TEST_DIRNAME/../scripts/chess-practice"
  args_file="$BATS_TEST_TMPDIR/args"
  trainer="$BATS_TEST_TMPDIR/tactics-trainer"

  cat >"$trainer" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "$@" >"$TACTICS_TRAINER_ARGS_FILE"
exit 64
EOF
  chmod +x "$trainer"

  export TACTICS_TRAINER_ARGS_FILE="$args_file"
  export TACTICS_TRAINER_BIN="$trainer"
  export CHESS_PRACTICE_ONCE=1
  export CHESS_PRACTICE_NO_CLEAR=1
}

@test "shows wrapper help without invoking tactics trainer" {
  run "$wrapper" --help

  [ "$status" -eq 0 ]
  [[ "$output" == *"Usage: scripts/chess-practice"* ]]
  [ ! -e "$args_file" ]
}

@test "forwards options and maps singular tag option" {
  run "$wrapper" --rating=600-1200 --tag mateIn1

  [ "$status" -eq 64 ]
  [ "$(cat "$args_file")" = $'--rating=600-1200\n--tags\nmateIn1' ]
}

@test "defaults to beginner rating range with no options" {
  run "$wrapper"

  [ "$status" -eq 64 ]
  [ "$(cat "$args_file")" = "--rating=600-1200" ]
}

@test "clears the screen before running tactics trainer" {
  unset CHESS_PRACTICE_NO_CLEAR
  bin_dir="$BATS_TEST_TMPDIR/bin"
  order_file="$BATS_TEST_TMPDIR/order"
  mkdir "$bin_dir"

  cat >"$bin_dir/clear" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' clear >>"$TACTICS_TRAINER_ORDER_FILE"
EOF
  chmod +x "$bin_dir/clear"

  cat >"$trainer" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' trainer >>"$TACTICS_TRAINER_ORDER_FILE"
exit 64
EOF
  chmod +x "$trainer"

  export TACTICS_TRAINER_ORDER_FILE="$order_file"

  PATH="$bin_dir:/usr/bin:/bin" run "$wrapper"

  [ "$status" -eq 64 ]
  [ "$(cat "$order_file")" = $'clear\ntrainer' ]
}

@test "builds the checkout binary once before installed tactics trainer" {
  unset TACTICS_TRAINER_BIN
  bin_dir="$BATS_TEST_TMPDIR/bin"
  cargo_args_file="$BATS_TEST_TMPDIR/cargo-args"
  mkdir "$bin_dir"

  cat >"$bin_dir/cargo" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "$@" >"$TACTICS_TRAINER_CARGO_ARGS_FILE"
mkdir -p "$CARGO_TARGET_DIR/debug"
cat >"$CARGO_TARGET_DIR/debug/tactics-trainer" <<'INNER_EOF'
#!/usr/bin/env bash
printf '%s\n' "$@" >"$TACTICS_TRAINER_ARGS_FILE"
exit 64
INNER_EOF
chmod +x "$CARGO_TARGET_DIR/debug/tactics-trainer"
EOF
  chmod +x "$bin_dir/cargo"

  cat >"$bin_dir/tactics-trainer" <<'EOF'
#!/usr/bin/env bash
exit 65
EOF
  chmod +x "$bin_dir/tactics-trainer"

  export CARGO_TARGET_DIR="$BATS_TEST_TMPDIR/target"
  export TACTICS_TRAINER_CARGO_ARGS_FILE="$cargo_args_file"

  PATH="$bin_dir:/usr/bin:/bin" run "$wrapper"

  [ "$status" -eq 64 ]
  repo_root=$(cd -- "$BATS_TEST_DIRNAME/.." && pwd)
  [ "$(cat "$cargo_args_file")" = "build
--quiet
--manifest-path
$repo_root/Cargo.toml
--bin
tactics-trainer" ]
  [ "$(cat "$args_file")" = "--rating=600-1200" ]
}

@test "reuses the checkout binary across practice attempts" {
  unset TACTICS_TRAINER_BIN
  unset CHESS_PRACTICE_ONCE
  bin_dir="$BATS_TEST_TMPDIR/bin"
  cargo_args_file="$BATS_TEST_TMPDIR/cargo-args"
  attempts_file="$BATS_TEST_TMPDIR/attempts"
  mkdir "$bin_dir"

  cat >"$bin_dir/cargo" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "$@" >>"$TACTICS_TRAINER_CARGO_ARGS_FILE"
mkdir -p "$CARGO_TARGET_DIR/debug"
cat >"$CARGO_TARGET_DIR/debug/tactics-trainer" <<'INNER_EOF'
#!/usr/bin/env bash
attempts=$(cat "$TACTICS_TRAINER_ATTEMPTS_FILE" 2>/dev/null || true)
attempts=${attempts:-0}
attempts=$((attempts + 1))
printf '%s\n' "$attempts" >"$TACTICS_TRAINER_ATTEMPTS_FILE"
exit 64
INNER_EOF
chmod +x "$CARGO_TARGET_DIR/debug/tactics-trainer"
EOF
  chmod +x "$bin_dir/cargo"

  export CARGO_TARGET_DIR="$BATS_TEST_TMPDIR/target"
  export CHESS_PRACTICE_MAX_RUNS=2
  export CHESS_PRACTICE_RETRY_DELAY=0
  export TACTICS_TRAINER_ATTEMPTS_FILE="$attempts_file"
  export TACTICS_TRAINER_CARGO_ARGS_FILE="$cargo_args_file"

  PATH="$bin_dir:/usr/bin:/bin" run "$wrapper"

  [ "$status" -eq 64 ]
  [ "$(cat "$attempts_file")" = "2" ]
  [ "$(wc -l <"$cargo_args_file" | tr -d ' ')" = "6" ]
  [ "$(cat "$args_file" 2>/dev/null || true)" = "" ]
}

@test "continues after tactics trainer exits with an error" {
  unset CHESS_PRACTICE_ONCE
  export CHESS_PRACTICE_MAX_RUNS=2
  export CHESS_PRACTICE_RETRY_DELAY=0
  attempts_file="$BATS_TEST_TMPDIR/attempts"

  cat >"$trainer" <<'EOF'
#!/usr/bin/env bash
attempts=$(cat "$TACTICS_TRAINER_ATTEMPTS_FILE" 2>/dev/null || true)
attempts=${attempts:-0}
attempts=$((attempts + 1))
printf '%s\n' "$attempts" >"$TACTICS_TRAINER_ATTEMPTS_FILE"
exit 64
EOF
  chmod +x "$trainer"

  export TACTICS_TRAINER_ATTEMPTS_FILE="$attempts_file"

  run "$wrapper"

  [ "$status" -eq 64 ]
  [ "$(cat "$attempts_file")" = "2" ]
  [[ "$output" == *"tactics-trainer exited with status 64; retrying"* ]]
}

@test "rejects singular tag option without a value" {
  run "$wrapper" --tag

  [ "$status" -eq 64 ]
  [[ "$output" == *"--tag requires a value"* ]]
}
