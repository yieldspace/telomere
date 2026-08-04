#!/usr/bin/env bash
# Sample interpreter-handler time without adding any dispatch-path instrumentation.
#
# Examples:
#   tools/baseline/attribute-time.sh --seconds 20 -- \
#     tools/baseline/loop-50m.wasm run 50000000
#   tools/baseline/attribute-time.sh --no-build --binary /tmp/telomere-cli -- \
#     tools/baseline/loop-50m.wasm run 50000000
#
# The generated report intentionally calls the numbers flat *observations*, not
# timings.  ``sample``/``perf`` are attribution-only inputs; baseline timing
# always comes from tools/measure-interpreter-baseline.py's release cells.

set -euo pipefail

usage() {
    cat <<'EOF'
Usage: tools/baseline/attribute-time.sh [options] -- <telomere-cli arguments>

Builds the release-profiling telomere-cli unless --no-build is given, samples
the spawned interpreter process with the platform profiler, and writes a flat
handler/family report plus the raw profiler artifact.

Options:
  --seconds N     Sampling duration in seconds (default: 20)
  --out PATH      Report path (default: target/baseline/attribution/<stamp>.txt)
  --binary PATH   CLI binary (default: target/release-profiling/telomere-cli)
  --no-build      Do not build the release-profiling binary first
  -h, --help      Show this help

The command after -- is passed to telomere-cli.  Use a workload that remains
alive for the whole sampling interval; a short or failing workload is refused.
EOF
}

seconds=20
binary="target/release-profiling/telomere-cli"
out=""
build=true

while (($#)); do
    case "$1" in
        --seconds)
            [[ $# -ge 2 ]] || { echo "--seconds requires a value" >&2; exit 2; }
            seconds="$2"
            shift 2
            ;;
        --out)
            [[ $# -ge 2 ]] || { echo "--out requires a path" >&2; exit 2; }
            out="$2"
            shift 2
            ;;
        --binary)
            [[ $# -ge 2 ]] || { echo "--binary requires a path" >&2; exit 2; }
            binary="$2"
            shift 2
            ;;
        --no-build)
            build=false
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        --)
            shift
            break
            ;;
        *)
            echo "unknown option: $1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

[[ "$seconds" =~ ^[1-9][0-9]*$ ]] || {
    echo "--seconds must be a positive integer" >&2
    exit 2
}
[[ $# -gt 0 ]] || {
    echo "missing telomere-cli arguments after --" >&2
    usage >&2
    exit 2
}

if [[ "$build" == true ]]; then
    cargo build --locked --profile release-profiling -p telomere-cli
fi
[[ -x "$binary" ]] || {
    echo "missing executable: $binary" >&2
    exit 1
}

if [[ -z "$out" ]]; then
    stamp="$(date -u +%Y%m%dT%H%M%SZ)-$(git rev-parse --short HEAD)"
    out="target/baseline/attribution/${stamp}.txt"
fi
[[ ! -e "$out" ]] || {
    echo "refusing to overwrite existing report: $out" >&2
    exit 1
}
mkdir -p "$(dirname "$out")"

stdout_path="${out}.stdout"
stderr_path="${out}.stderr"
symbols_path="${out}.symbols.tsv"
families_path="${out}.families.tsv"

"$binary" "$@" >"$stdout_path" 2>"$stderr_path" &
child_pid=$!

profile_status=0
workload_status=0
profile_kind=""
raw_profile=""
flat_profile=""

case "$(uname -s)" in
    Darwin)
        profile_kind="sample"
        raw_profile="${out}.sample.txt"
        set +e
        sample "$child_pid" "$seconds" -mayDie -file "$raw_profile"
        profile_status=$?
        wait "$child_pid"
        workload_status=$?
        set -e
        flat_profile="$raw_profile"
        ;;
    Linux)
        command -v perf >/dev/null 2>&1 || {
            echo "perf is required for Linux attribution" >&2
            exit 1
        }
        profile_kind="perf"
        raw_profile="${out}.perf.data"
        flat_profile="${out}.perf.txt"
        set +e
        perf record -F 999 -p "$child_pid" -o "$raw_profile" -- sleep "$seconds"
        profile_status=$?
        wait "$child_pid"
        workload_status=$?
        set -e
        if [[ "$profile_status" -eq 0 ]]; then
            perf report --stdio --no-children --sort=symbol -i "$raw_profile" >"$flat_profile"
        fi
        ;;
    *)
        echo "unsupported platform for sampling: $(uname -s)" >&2
        exit 1
        ;;
esac

if [[ "$profile_status" -ne 0 || "$workload_status" -ne 0 ]]; then
    echo "sampling or workload failed; refusing attribution report" >&2
    echo "profiler_status=$profile_status workload_status=$workload_status" >&2
    exit 1
fi

# The profiler views are flattened deliberately. Rust's tail-call dispatcher
# does not preserve a meaningful call-tree parent, so a hierarchy would make
# handler attribution look more precise than it is. The helper rejects raw
# textual occurrence counts: Darwin uses each sample line's leading count and
# Linux preserves perf's Overhead percentage as the weight.
python3 "$(dirname "$0")/profile_attribution.py" \
    --format "$profile_kind" \
    --input "$flat_profile" \
    --symbols-out "$symbols_path" \
    --families-out "$families_path"

{
    printf 'profile_kind=%s\n' "$profile_kind"
    printf 'sampling_seconds=%s\n' "$seconds"
    printf 'binary=%s\n' "$binary"
    printf 'raw_profile=%s\n' "$raw_profile"
    printf 'flat_profile=%s\n' "$flat_profile"
    printf 'workload_stdout=%s\n' "$stdout_path"
    printf 'workload_stderr=%s\n' "$stderr_path"
    printf '\n# HandlerLayoutGroup family table (weighted flat profiler samples)\n'
    cat "$families_path"
    printf '\n# Leaf handler weights (not timings)\n'
    cat "$symbols_path"
} >"$out"

printf '%s\n' "$out"
