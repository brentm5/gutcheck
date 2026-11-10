#!/usr/bin/env bash
# Manual evaluation suite: runs the release binary against the real Jev API.
# Run by hand only, with `fnox exec -- mise run eval`. Never part of CI or
# `mise run test`. See docs/testing.md ("Manual evaluations").
set -uo pipefail

if [[ -z "${TYPESAFE_API_KEY:-}" ]]; then
  echo "error: TYPESAFE_API_KEY is not set; run the suite with: fnox exec -- mise run eval" >&2
  exit 1
fi

BIN="${GUTCHECK_BIN:-target/release/gutcheck}"
if [[ ! -x "$BIN" ]]; then
  echo "error: $BIN not found; build it with: mise run build" >&2
  exit 1
fi

passed=0
failed=0
out_file=$(mktemp)
err_file=$(mktemp)
trap 'rm -f "$out_file" "$err_file"' EXIT

# Replace the real key with a placeholder, so failure output never shows it.
redact() {
  local text=$1
  printf '%s' "${text//"$TYPESAFE_API_KEY"/<redacted>}"
}

# run [VAR=value ...] -- ARGS... [< stdin]
# Runs the binary and sets $status, $out and $err. Leading VAR=value words are
# set in the binary's environment only. $cmd describes the call for failure
# output.
run() {
  local -a envs=()
  while [[ $# -gt 0 && $1 != "--" ]]; do
    envs+=("$1")
    shift
  done
  shift
  cmd="${envs[*]:+${envs[*]} }gutcheck$(printf ' %q' "$@")"
  env ${envs[@]+"${envs[@]}"} "$BIN" "$@" >"$out_file" 2>"$err_file"
  status=$?
  out=$(<"$out_file")
  err=$(<"$err_file")
  out_lines=$(wc -l <"$out_file" | tr -d ' ')
  err_lines=$(wc -l <"$err_file" | tr -d ' ')
}

# Problems found in the current case; report clears them.
problems=()
fail() { problems+=("$1"); }

report() {
  local name=$1
  if [[ ${#problems[@]} -eq 0 ]]; then
    echo "PASS  $name"
    passed=$((passed + 1))
  else
    echo "FAIL  $name"
    local p
    for p in "${problems[@]}"; do
      echo "      problem: $p"
    done
    echo "      command: $(redact "$cmd")"
    echo "      exit:    $status"
    echo "      stdout:  $(redact "$out")"
    echo "      stderr:  $(redact "$err")"
    failed=$((failed + 1))
  fi
  problems=()
}

# Checks shared by every successful call: exit 0, one stdout line, no stderr.
expect_success() {
  [[ $status -eq 0 ]] || fail "expected exit 0"
  [[ $out_lines -eq 1 && -n $out ]] || fail "expected one line on stdout"
  [[ -z $err ]] || fail "expected nothing on stderr"
}

# Checks shared by every error: exit 1, no stdout, one stderr line.
expect_error() {
  [[ $status -eq 1 ]] || fail "expected exit 1"
  [[ -z $out ]] || fail "expected nothing on stdout"
  [[ $err_lines -eq 1 && -n $err ]] || fail "expected one line on stderr"
}

# in_range VALUE MIN MAX : VALUE is a number with MIN <= VALUE <= MAX.
in_range() {
  jq -en --arg v "$1" --argjson min "$2" --argjson max "$3" \
    '($v | tonumber? // null) as $n | $n != null and $n >= $min and $n <= $max' >/dev/null 2>&1
}

# json_check FILTER : stdout is valid JSON for which FILTER is true.
json_check() {
  jq -e "$1" <<<"$out" >/dev/null 2>&1
}

YES_STATE="The sky is blue and water is wet."
NOUL_Q="This statement is true"
REFUND_STATE="I was charged twice for my subscription this month. Please refund the extra charge."
CHOICE_ARGS=(-o billing -o shipping -o technical)
CHOICE_Q="Which team should handle this message?"
FURIOUS_STATE="THIS IS UNACCEPTABLE!!! I have asked FIVE times and nobody fixes it. I am absolutely furious and I am done with you people!"
CALM_STATE="Hi, no rush at all. Whenever you have a moment, could you tell me your opening hours? Thanks so much, have a lovely day."
SCORE_ARGS=(-l Calm -l Annoyed -l Angry)
SCORE_Q="How angry is the customer?"
# Three levels: scores run from 0 to 2, and the top range starts at 3 - 1.5.
SCORE_TOP=2
SCORE_HIGH=1.5

echo "== Contract"

run -- ask noul "$NOUL_Q" --state "$YES_STATE"
expect_success
in_range "$out" 0 1 || fail "expected a number in [0,1]"
report "noul: plain output with --state"

run -- ask noul "$NOUL_Q" --json <<<"$YES_STATE"
expect_success
json_check '.type == "noul" and (.noul | type == "number")' || fail "expected JSON with type noul and a numeric noul"
report "noul: --json with state on stdin"

run -- ask noul "$NOUL_Q" --boolean --state "$YES_STATE"
expect_success
[[ $out == "true" || $out == "false" ]] || fail "expected true or false"
report "noul: --boolean"

run -- ask choice "$CHOICE_Q" "${CHOICE_ARGS[@]}" --state "$REFUND_STATE"
expect_success
[[ $out == "billing" || $out == "shipping" || $out == "technical" ]] || fail "expected one of the given keys"
report "choice: plain output with --state"

run -- ask choice "$CHOICE_Q" "${CHOICE_ARGS[@]}" --json <<<"$REFUND_STATE"
expect_success
json_check '.type == "choice"
  and (.choice | IN("billing", "shipping", "technical"))
  and (.probabilities | type == "object")
  and (.confidence | type == "number")' ||
  fail "expected JSON with type choice, a given key, probabilities and confidence"
report "choice: --json with state on stdin"

run -- ask score "$SCORE_Q" "${SCORE_ARGS[@]}" --state "$FURIOUS_STATE"
expect_success
in_range "$out" 0 "$SCORE_TOP" || fail "expected a number in [0,$SCORE_TOP]"
report "score: plain output with --state"

run -- ask score "$SCORE_Q" "${SCORE_ARGS[@]}" --json <<<"$FURIOUS_STATE"
expect_success
json_check '.type == "score"
  and (.score | type == "number")
  and (.legend | type == "object")
  and (.probabilities | type == "object")
  and (.confidence | type == "number")' ||
  fail "expected JSON with type score, score, legend, probabilities and confidence"
report "score: --json with state on stdin"

run -- auth validate
[[ $status -eq 0 ]] || fail "expected exit 0"
[[ -z $out && -z $err ]] || fail "expected no output"
report "auth validate: real key accepted"

echo "== Answer sanity"

run -- ask noul "$NOUL_Q" --state "$YES_STATE"
expect_success
in_range "$out" 0.8 1 || fail "expected a probability above 0.8"
report "noul: obvious yes is above 0.8"

run -- ask noul "$NOUL_Q" --state "The sun rises in the west and fish breathe air through lungs."
expect_success
in_range "$out" 0 0.2 || fail "expected a probability below 0.2"
report "noul: obvious no is below 0.2"

run -- ask choice "$CHOICE_Q" "${CHOICE_ARGS[@]}" --state "$REFUND_STATE"
expect_success
[[ $out == "billing" ]] || fail "expected billing"
report "choice: refund request picks billing"

run -- ask score "$SCORE_Q" "${SCORE_ARGS[@]}" --state "$FURIOUS_STATE"
expect_success
in_range "$out" "$SCORE_HIGH" "$SCORE_TOP" || fail "expected a score of at least $SCORE_HIGH"
report "score: furious message scores at the top"

run -- ask score "$SCORE_Q" "${SCORE_ARGS[@]}" --state "$CALM_STATE"
expect_success
in_range "$out" 0 0.5 || fail "expected a score of at most 0.5"
report "score: calm message scores at the bottom"

echo "== Real API errors"

run TYPESAFE_API_KEY=invalid -- ask noul "$NOUL_Q" --state "$YES_STATE"
expect_error
[[ $err == *[Ii]nvalid* && $err == *"API key"* ]] || fail "expected a message saying the API key is invalid"
report "401: ask with an invalid key"

run TYPESAFE_API_KEY=invalid -- auth validate
expect_error
[[ $err == *[Ii]nvalid* && $err == *"API key"* ]] || fail "expected a message saying the API key is invalid"
report "401: auth validate with an invalid key"

run -- --model does-not-exist ask noul "$NOUL_Q" --state "$YES_STATE"
expect_error
[[ $err == *model* ]] || fail "expected a message naming the model problem"
[[ $err != *"{"* ]] || fail "expected a message, not a raw response"
report "invalid model: --model does-not-exist"

echo
echo "$passed passed, $failed failed"
[[ $failed -eq 0 ]]
