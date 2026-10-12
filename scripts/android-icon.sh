#!/usr/bin/env bash
# Puts Tackly's launcher icon into the Android project that `dx build` made,
# and builds the APK again (Gradle only, no Rust). dx has no setting for the
# icon: it always writes its template's.
#
#   scripts/android-icon.sh <dx's android project>   (target/dx/tackly/debug/android/app)
#
# Prints the path of the APK.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
project="${1:?usage: android-icon.sh <the Android project dx made>}"
icons="$root/apps/app/icons/android"
res="$project/app/src/main/res"

# Before Android 8: the PNGs. dx's template has WebP files of the same name.
for density in mdpi hdpi xhdpi xxhdpi xxxhdpi; do
  rm -f "$res/mipmap-$density/ic_launcher.webp"
  mkdir -p "$res/mipmap-$density"
  cp "$icons/mipmap-$density/ic_launcher.png" "$res/mipmap-$density/ic_launcher.png"
done
# Android 8 and later: the adaptive icon, from two vectors.
cp "$icons/ic_launcher_background.xml" "$res/drawable/ic_launcher_background.xml"
cp "$icons/ic_launcher_foreground.xml" "$res/drawable-v24/ic_launcher_foreground.xml"

(cd "$project" && ./gradlew --quiet assembleDebug)

# dx writes its WebP files again on its next build, and next to our PNGs of the
# same name they would be duplicate resources: leave the project as dx wants it.
for density in mdpi hdpi xhdpi xxhdpi xxxhdpi; do
  rm -f "$res/mipmap-$density/ic_launcher.png"
done
echo "$project/app/build/outputs/apk/debug/app-debug.apk"
