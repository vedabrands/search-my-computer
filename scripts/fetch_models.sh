#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LOCK_FILE="${SCRIPT_DIR}/../models.lock"
OUTPUT_DIR="${SCRIPT_DIR}/../models"
TARGET_MODEL="${1:-all}"

if [ ! -f "$LOCK_FILE" ]; then
    echo "Error: models.lock not found at $LOCK_FILE" >&2
    exit 1
fi

if command -v jq >/dev/null 2>&1; then
    GET_KEYS_CMD="jq -r '.models | keys[]' '$LOCK_FILE'"
elif command -v node >/dev/null 2>&1; then
    GET_KEYS_CMD="cat '$LOCK_FILE' | node -e \"const d = JSON.parse(require('fs').readFileSync(0, 'utf-8')); console.log(Object.keys(d.models).join('\n'))\""
elif command -v python3 >/dev/null 2>&1; then
    GET_KEYS_CMD="python3 -c \"import json; print('\n'.join(json.load(open('$LOCK_FILE'))['models'].keys()))\""
elif command -v python >/dev/null 2>&1; then
    GET_KEYS_CMD="python -c \"import json; print('\n'.join(json.load(open('$LOCK_FILE'))['models'].keys()))\""
else
    # basic grep fallback
    GET_KEYS_CMD="grep -o '\"[a-zA-Z0-9._-]*\": {' '$LOCK_FILE' | tr -d '\"{ :' | grep -v 'files'"
fi

if [ "$TARGET_MODEL" = "all" ]; then
    MODEL_KEYS=$(eval "$GET_KEYS_CMD")
else
    MODEL_KEYS="$TARGET_MODEL"
fi

for model in $MODEL_KEYS; do
    echo "=== Fetching Model: $model ==="
    MODEL_DIR="${OUTPUT_DIR}/${model}"
    mkdir -p "$MODEL_DIR"

    if command -v jq >/dev/null 2>&1; then
        FILES=$(jq -r ".models[\"$model\"].files | keys[]" "$LOCK_FILE")
    elif command -v node >/dev/null 2>&1; then
        FILES=$(cat "$LOCK_FILE" | node -e "const d = JSON.parse(require('fs').readFileSync(0, 'utf-8')); console.log(Object.keys(d.models['$model'].files).join('\n'))")
    elif command -v python3 >/dev/null 2>&1; then
        FILES=$(python3 -c "import json; print('\n'.join(json.load(open('$LOCK_FILE'))['models']['$model']['files'].keys()))")
    elif command -v python >/dev/null 2>&1; then
        FILES=$(python -c "import json; print('\n'.join(json.load(open('$LOCK_FILE'))['models']['$model']['files'].keys()))")
    else
        FILES="model.onnx tokenizer.json"
    fi

    for fname in $FILES; do
        if command -v jq >/dev/null 2>&1; then
            URL=$(jq -r ".models[\"$model\"].files[\"$fname\"].url" "$LOCK_FILE")
            SHA256=$(jq -r ".models[\"$model\"].files[\"$fname\"].sha256" "$LOCK_FILE")
        elif command -v node >/dev/null 2>&1; then
            URL=$(cat "$LOCK_FILE" | node -e "const d = JSON.parse(require('fs').readFileSync(0, 'utf-8')); console.log(d.models['$model'].files['$fname'].url)")
            SHA256=$(cat "$LOCK_FILE" | node -e "const d = JSON.parse(require('fs').readFileSync(0, 'utf-8')); console.log(d.models['$model'].files['$fname'].sha256)")
        elif command -v python3 >/dev/null 2>&1; then
            URL=$(python3 -c "import json; print(json.load(open('$LOCK_FILE'))['models']['$model']['files']['$fname']['url'])")
            SHA256=$(python3 -c "import json; print(json.load(open('$LOCK_FILE'))['models']['$model']['files']['$fname']['sha256'])")
        elif command -v python >/dev/null 2>&1; then
            URL=$(python -c "import json; print(json.load(open('$LOCK_FILE'))['models']['$model']['files']['$fname']['url'])")
            SHA256=$(python -c "import json; print(json.load(open('$LOCK_FILE'))['models']['$model']['files']['$fname']['sha256'])")
        fi

        DEST_FILE="${MODEL_DIR}/${fname}"
        DOWNLOAD_NEEDED=1

        if [ -f "$DEST_FILE" ]; then
            if command -v sha256sum >/dev/null 2>&1; then
                EXISTING_HASH=$(sha256sum "$DEST_FILE" | awk '{print $1}')
            else
                EXISTING_HASH=$(shasum -a 256 "$DEST_FILE" | awk '{print $1}')
            fi

            if [ "$EXISTING_HASH" = "$SHA256" ]; then
                echo "  [OK] $fname exists and SHA-256 matches ($EXISTING_HASH)"
                DOWNLOAD_NEEDED=0
            else
                echo "  [WARN] $fname hash mismatch, re-downloading..."
            fi
        fi

        if [ "$DOWNLOAD_NEEDED" -eq 1 ]; then
            echo "  [DOWNLOADING] $fname from $URL ..."
            TMP_FILE="${DEST_FILE}.tmp"
            if command -v curl >/dev/null 2>&1; then
                curl -sSL -o "$TMP_FILE" "$URL"
            elif command -v wget >/dev/null 2>&1; then
                wget -q -O "$TMP_FILE" "$URL"
            else
                echo "Error: neither curl nor wget found" >&2
                exit 1
            fi

            if command -v sha256sum >/dev/null 2>&1; then
                NEW_HASH=$(sha256sum "$TMP_FILE" | awk '{print $1}')
            else
                NEW_HASH=$(shasum -a 256 "$TMP_FILE" | awk '{print $1}')
            fi

            if [ "$NEW_HASH" != "$SHA256" ]; then
                rm -f "$TMP_FILE"
                echo "Error: SHA-256 verification failed for $fname! Expected: $SHA256, Got: $NEW_HASH" >&2
                exit 1
            fi

            mv "$TMP_FILE" "$DEST_FILE"
            echo "  [VERIFIED] $fname downloaded and verified successfully."
        fi
    done
done

echo "=== All requested models verified successfully ==="
