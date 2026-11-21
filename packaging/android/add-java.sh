#!/usr/bin/env bash
# Adds the Java part (MainActivity: system file picker) to the APK built by cargo-apk,
# then re-aligns and re-signs it. cargo-apk cannot compile Java sources itself.
#   packaging/android/add-java.sh <apk> <keystore> <keystore-password>
set -euo pipefail
APK=$(cd "$(dirname "$1")" && pwd)/$(basename "$1"); KS=$2; PASS=$3
SDK=${ANDROID_HOME:?ANDROID_HOME not set}
BT=$(ls -d "$SDK"/build-tools/* | sort -V | tail -1)
JAR=$(ls -d "$SDK"/platforms/android-* | sort -V | grep -E 'android-[0-9]+$' | tail -1)/android.jar
SRC=$(cd "$(dirname "$0")/../../crates/lightpdf-studio/android/java" && pwd)
W=$(mktemp -d)
javac -source 11 -target 11 -nowarn -classpath "$JAR" -d "$W/classes" $(find "$SRC" -name '*.java') 2>&1 | grep -v '^warning' || true
"$BT/d8" --release --min-api 26 --lib "$JAR" --output "$W" $(find "$W/classes" -name '*.class')
(cd "$W" && zip -q "$APK" classes.dex)
"$BT/zipalign" -f -p 4 "$APK" "$W/aligned.apk"
"$BT/apksigner" sign --ks "$KS" --ks-pass "pass:$PASS" --out "$APK" "$W/aligned.apk"
rm -rf "$W"
