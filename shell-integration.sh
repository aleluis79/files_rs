#!/usr/bin/env sh

# Source this file from zsh/bash to enable automatic cd after exiting ncrs.
# Example:
#   source /home/alejandro/proyectos/files/shell-integration.sh
# Then run:
#   ncrs

ncrs() {
  local tmp
  tmp="$(mktemp -t ncrs-chdir.XXXXXX)" || return 1

  NCRS_CHDIR_FILE="$tmp" cargo run --quiet --manifest-path /home/alejandro/proyectos/files/Cargo.toml
  local app_status=$?

  if [ -s "$tmp" ]; then
    local dest
    dest="$(head -n 1 "$tmp")"
    if [ -n "$dest" ] && [ -d "$dest" ]; then
      cd "$dest" || return $app_status
    fi
  fi

  rm -f "$tmp"
  return $app_status
}
