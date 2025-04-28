#!/bin/bash
TARGET="$1"

if [ -z "$TARGET" ]; then
  echo "No target path provided."
  exit 1
fi

source ~/.SearchFunctions/search_in_file_path_and_content

TARGET_REAL=$(realpath "$TARGET")

cd "$TARGET_REAL" || exit 1
ugf-plus
