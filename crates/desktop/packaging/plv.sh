#!/bin/sh
# Installed as /usr/bin/plv by the .deb: runs the Plover binary as the plv CLI.
PLOVER_AS_CLI=1 exec /usr/bin/plover "$@"
