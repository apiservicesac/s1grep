#!/bin/sh
# Runs development tooling inside Docker so nothing is installed on the host.
#   ./dev.sh cargo <args>        Rust toolchain (rust:1-trixie), cargo cache kept in .cache/
#   ./dev.sh windows build --release -p s1grep   cross-compiled s1grep.exe (target/x86_64-pc-windows-gnu)
#   ./dev.sh export uv sync      Python environment of the model export tool (tools/model-export)
#   ./dev.sh export python -m model_export <command>
# The export tool reads Hugging Face downloads from $MODEL_CACHE (default ~/.cache/huggingface).
set -e
cd "$(dirname "$0")"
PROJECT="$PWD"
MODEL_CACHE="${MODEL_CACHE:-$HOME/.cache/huggingface}"
TERMINAL=""; [ -t 0 ] && TERMINAL="-it"
mkdir -p .cache/cargo .cache/uv
BASE="docker run --rm $TERMINAL -u $(id -u):$(id -g) -e HOME=/tmp -e USER=developer -v $PROJECT:/project"
TOOL="$1"; shift
case "$TOOL" in
  cargo)
    exec $BASE -e CARGO_HOME=/project/.cache/cargo -e XDG_CACHE_HOME=/project/.cache/xdg -e CARGO_TARGET_DIR=/project/target -w /project \
      rust:1-trixie cargo "$@" ;;
  windows)
    docker build -q -t s1grep-cross-windows -f docker/cross-windows.Dockerfile docker >/dev/null
    exec $BASE -e CARGO_HOME=/project/.cache/cargo -e XDG_CACHE_HOME=/project/.cache/xdg -e CARGO_TARGET_DIR=/project/target -w /project \
      s1grep-cross-windows cargo "$@" --target x86_64-pc-windows-gnu ;;
  export)
    if [ "$1" = "uv" ]; then
      shift
      exec $BASE -e UV_CACHE_DIR=/project/.cache/uv -w /project/tools/model-export python:3.13-bookworm \
        sh -c "pip install -q --user uv && ~/.local/bin/uv $*"
    fi
    exec $BASE -e HF_HOME=/models -e HF_HUB_OFFLINE=1 -e PYTHONUNBUFFERED=1 -v "$MODEL_CACHE":/models -w /project/tools/model-export \
      python:3.13-bookworm /project/tools/model-export/.venv/bin/"$@" ;;
  *)
    echo "usage: ./dev.sh cargo|windows|export ..." >&2; exit 2 ;;
esac
