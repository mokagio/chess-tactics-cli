#!/usr/bin/env bats

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

@test "falls back to cargo run when tactics trainer is not installed" {
  unset TACTICS_TRAINER_BIN
  bin_dir="$BATS_TEST_TMPDIR/bin"
  mkdir "$bin_dir"

  cat >"$bin_dir/cargo" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "$@" >"$TACTICS_TRAINER_ARGS_FILE"
exit 64
EOF
  chmod +x "$bin_dir/cargo"

  PATH="$bin_dir:/usr/bin:/bin" run "$wrapper"

  [ "$status" -eq 64 ]
  [ "$(cat "$args_file")" = $'run\n--quiet\n--bin\ntactics-trainer\n--\n--rating=600-1200' ]
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
