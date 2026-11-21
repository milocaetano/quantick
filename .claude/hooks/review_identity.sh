#!/bin/sh
set -eu
for interpreter in python3 python; do
    if "$interpreter" -c 'import sys; assert sys.version_info >= (3, 12)' >/dev/null 2>&1; then
        exec "$interpreter" "$(dirname "$0")/review_identity.py" "$@"
    fi
done
echo 'Python 3.12 or newer is required for review identity.' >&2
exit 1
