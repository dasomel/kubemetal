#!/usr/bin/env bash
# D43: isolate the managed VM; cost: reserved names unavailable; escape: a valid override.
_colima_profile_file="$(dirname "${BASH_SOURCE[0]}")/colima-profile.txt"
if [[ ! -r "$_colima_profile_file" ]]; then
  echo "Missing managed Colima profile source: $_colima_profile_file" >&2
  exit 1
fi
_colima_default="$(cat "$_colima_profile_file")" || exit 1
_colima_default="${_colima_default#"${_colima_default%%[![:space:]]*}"}"
_colima_default="${_colima_default%"${_colima_default##*[![:space:]]}"}"
if [[ -z "$_colima_default" ]]; then
  echo "Empty managed Colima profile source: $_colima_profile_file" >&2
  exit 1
fi
KUBEMETAL_COLIMA_PROFILE="${KUBEMETAL_COLIMA_PROFILE-$_colima_default}"
KUBEMETAL_COLIMA_PROFILE="${KUBEMETAL_COLIMA_PROFILE#"${KUBEMETAL_COLIMA_PROFILE%%[![:space:]]*}"}"
KUBEMETAL_COLIMA_PROFILE="${KUBEMETAL_COLIMA_PROFILE%"${KUBEMETAL_COLIMA_PROFILE##*[![:space:]]}"}"
if [[ ! "$KUBEMETAL_COLIMA_PROFILE" =~ ^[a-z0-9][a-z0-9-]*$ ]] ||
  [[ "$KUBEMETAL_COLIMA_PROFILE" == default || "$KUBEMETAL_COLIMA_PROFILE" == colima || "$KUBEMETAL_COLIMA_PROFILE" == colima-* ]]; then
  echo "Invalid KUBEMETAL_COLIMA_PROFILE: '$KUBEMETAL_COLIMA_PROFILE'; expected [a-z0-9][a-z0-9-]*; default, colima and colima-* are reserved" >&2
  exit 1
fi
COLIMA_CONTEXT="colima-${KUBEMETAL_COLIMA_PROFILE}"
DOCKER_CONTEXT="$COLIMA_CONTEXT"
export KUBEMETAL_COLIMA_PROFILE COLIMA_CONTEXT DOCKER_CONTEXT
unset _colima_profile_file _colima_default
