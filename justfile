# marq - macOS markdown viewer

# Build and run marq with test doc. Pass --debug to run in foreground with logs.
run-local *FLAGS:
    #!/usr/bin/env bash
    swift build
    if echo "{{FLAGS}}" | grep -q -- "--debug"; then
        .build/debug/marq examples/test.md
    else
        nohup .build/debug/marq examples/test.md &>/dev/null &
        sleep 0.5
        osascript -e 'tell application "System Events" to set frontmost of (first process whose unix id is '"$!"') to true'
    fi
