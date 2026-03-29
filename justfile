# marq - macOS markdown viewer

# Build and run marq with test doc. Pass --debug to run in foreground with logs.
run-local *FLAGS:
    #!/usr/bin/env bash
    swift build
    if echo "{{FLAGS}}" | grep -q -- "--debug"; then
        .build/debug/marq examples/test.md
    else
        .build/debug/marq examples/test.md &
    fi
