#!/usr/bin/env bash
set -euo pipefail

qualification_dir="$(cd "$(dirname "$0")" && pwd)"
cuda_crate_dir="$(cd "$qualification_dir/.." && pwd)"
cuda_output_dir="${1:?Usage: qualify-nvrtc.sh OUTPUT_DIRECTORY [COMPUTE_CAPABILITY ...]}"
shift
mkdir -p "$cuda_output_dir"
cuda_output_dir="$(cd "$cuda_output_dir" && pwd)"
cuda_image="${CUDA_NVRTC_IMAGE:-rust:1-bookworm}"
cuda_image_id="$(docker image inspect "$cuda_image" --format '{{.Id}}')"
if [ "$#" -eq 0 ]; then
    set -- 70 80 90 120
fi

# The existing image is used by immutable ID. Source is read-only; only the
# requested qualification output directory is writable on the host. Downloaded
# NVRTC libraries live in tmpfs and disappear when this container exits.
docker run --rm --pull never --read-only --cap-drop ALL \
    --security-opt no-new-privileges --pids-limit 128 --memory 2g --cpus 2 \
    --user "$(id -u):$(id -g)" \
    --tmpfs /tmp:rw,exec,nosuid,size=512m \
    --mount "type=bind,src=$cuda_crate_dir,dst=/source,readonly" \
    --mount "type=bind,src=$cuda_output_dir,dst=/output" \
    --env "CUDA_NVRTC_IMAGE_ID=$cuda_image_id" \
    "$cuda_image_id" python3 /source/qualification/compile_nvrtc.py \
    --crate /source --output /output --compute-capabilities "$@"
