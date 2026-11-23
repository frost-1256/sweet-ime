#!/usr/bin/env bash
set -euo pipefail

apk=${1:?pass the built Sweet IME APK}
package=io.github.sweetime
ime="$package/.SweetIme"

adb install -g "$apk"
adb logcat -c
adb shell input keyevent KEYCODE_WAKEUP
adb shell wm dismiss-keyguard
adb shell settings put secure show_ime_with_hard_keyboard 1
# The input method service registers the new IME shortly after the install returns.
for ((attempt = 0; attempt < 30; attempt++)); do
    if adb shell ime list -a -s | tr -d '\r' | grep -qx "$ime"; then
        break
    fi
    sleep 1
done
adb shell ime enable "$ime"
adb shell ime set "$ime"
[[ $(adb shell settings get secure default_input_method | tr -d '\r') == "$ime" ]]
adb shell am start -W -n "$package/.SettingsActivity"

adb shell uiautomator dump /sdcard/sweet-ime-smoke.xml
position=$(adb shell cat /sdcard/sweet-ime-smoke.xml | python3 -c '
import re
import sys
import xml.etree.ElementTree as ET

for field in ET.parse(sys.stdin).iter("node"):
    if field.get("package") == "io.github.sweetime" and field.get("class") == "android.widget.EditText":
        left, top, right, bottom = map(int, re.findall(r"\d+", field.get("bounds", "")))
        print((left + right) // 2, (top + bottom) // 2)
        break
else:
    sys.exit("Sweet IME settings did not show its test field")
')
read -r x y <<< "$position"
adb shell input tap "$x" "$y"

for ((attempt = 0; attempt < 30; attempt++)); do
    log=$(adb logcat -d -s SweetIME:I)
    if [[ "$log" == *"dictionary loaded: true"* ]]; then
        printf '%s\n' 'Sweet IME loaded its dictionary on the emulator'
        exit 0
    fi
    sleep 1
done
adb logcat -d -s SweetIME:V AndroidRuntime:E
printf '%s\n' 'Sweet IME did not load its dictionary' >&2
exit 1
