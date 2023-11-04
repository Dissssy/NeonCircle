#!/bin/sh

# This script is used to run the application in a loop.
# It is used to handle the daily restart of the application. 
# It is also used to notify the user if the application crashes.

# loop until the application has a return code of != 0
while true; do
    yt-dlp --update-to nightly
    # run the application
    cargo run --release    

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
