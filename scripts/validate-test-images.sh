#!/bin/zsh

set -euo pipefail

manifest="${1:-test-images/generated/manifest.tsv}"
image_dir="${manifest:h}"
checked=0
failed=0

while IFS=$'\t' read -r file_name expected_number shape font font_size prefix palette alignment; do
    if [[ "$file_name" == "file_name" ]]; then
        continue
    fi

    actual="$(tesseract "$image_dir/$file_name" stdout -l eng --psm 7 2>/dev/null)"
    if [[ "$actual" != *"$expected_number"* ]]; then
        print -r -- "FAIL\t$file_name\texpected=$expected_number\tactual=$actual"
        failed=$((failed + 1))
    fi
    checked=$((checked + 1))
done < "$manifest"

print -r -- "Validated $checked images; failures: $failed"
exit "$failed"
