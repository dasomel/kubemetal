#!/usr/bin/env bash
# D43: one profile source; the environment is the deliberate escape hatch.
COLIMA_PROFILE="${COLIMA_PROFILE:-$(cat "$(dirname "${BASH_SOURCE[0]}")/colima-profile.txt")}"
COLIMA_CONTEXT="colima-${COLIMA_PROFILE}"
export COLIMA_PROFILE COLIMA_CONTEXT
