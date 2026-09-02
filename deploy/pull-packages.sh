#!/bin/sh
# Compatibility no-op for an installed legacy publisher timer. pull-app.sh
# replaces any older deployed copy with this harmless script before the timer
# is removed from the production box. The frozen archives are served directly
# by Caddy and need no publisher process.
set -eu

echo "package publisher disabled: compatibility no-op"
exit 0
