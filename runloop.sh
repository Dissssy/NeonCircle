#!/bin/sh

# This script is used to run the application in a loop.
# It is used to handle the daily restart of the application. 
# It is also used to notify the user if the application crashes.

# set log level
export RUST_LOG="neon_circle=trace,common=trace,config_command=trace,lts=trace,national_debt=trace,voice_events=trace,music_commands=trace"

# loop until the application has a return code of != 0
while true; do
    # yt-dlp --update-to nightly
    python -m pip install --upgrade yt-dlp
    if [ "$1" = "release" ]; then
        echo "Running in release mode"
        cargo build --release --features experimental
        # if the build fails, echo an error message and use the last good build, otherwise move the built binary to ./bin
        if [ $? -ne 0 ]; then
            echo "Build failed, using last good build"
        else
            echo "Build successful, moving binary to ./bin"
            cp ./target/release/neon_circle ./data/bin/neon_circle
        fi
    else
        echo "Running in debug mode"
        cargo build --features experimental
        # if the build fails, echo an error message and use the last good build, otherwise move the built binary to ./bin
        if [ $? -ne 0 ]; then
            echo "Build failed, using last good build"
        else
            echo "Build successful, moving binary to ./bin"
            cp ./target/debug/neon_circle ./data/bin/neon_circle
        fi
    fi
    # current date&time
    datetime=$(date '+%Y-%m-%d %H:%M:%S')
    # run the application
    ./data/bin/neon_circle > ./logs/log_$datetime.log 2>&1

    # save the return code
    ret=$?

    # if the return code is 0, then sleep for 1 minute
    # else exit the loop and echo the return code as well as the current date
    if [ $ret -eq 3 ]; then
        echo EXIT NOW IF YOU WANT!
	sleep 1m
	echo TOO LATE
    else
        echo "Application exited with return code: $ret"
        echo "Date: $(date)"
        exit $ret
    fi
done
