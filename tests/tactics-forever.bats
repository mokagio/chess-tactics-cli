#!/usr/bin/env bats

setup() {
  wrapper="$BATS_TEST_DIRNAME/../scripts/tactics-forever"
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
}

@test "shows wrapper help without invoking tactics trainer" {
  run "$wrapper" --help

  [ "$status" -eq 0 ]
  [[ "$output" == *"Usage: scripts/tactics-forever"* ]]
  [ ! -e "$args_file" ]
}

@test "forwards options and maps singular tag option" {
  run "$wrapper" --rating=600-1200 --tag mateIn1

  [ "$status" -eq 64 ]
  [ "$(cat "$args_file")" = $'--rating=600-1200\n--tags\nmateIn1' ]
}

@test "rejects singular tag option without a value" {
  run "$wrapper" --tag

  [ "$status" -eq 64 ]
  [[ "$output" == *"--tag requires a value"* ]]
}
