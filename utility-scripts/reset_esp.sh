#!/bin/bash
fileDir=$(dirname "$0")
if [ "$fileDir" != '' ]; then
    fileDir="$fileDir/"
fi
esptool.py write_flash 37248 "${fileDir}hello"
