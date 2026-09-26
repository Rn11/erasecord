#!/bin/sh
# Draws assets/icon.png (the design of assets/icon.svg) with ImageMagick and
# generates the app icons from it: sh assets/make-icon.sh (run in app/).
set -e
convert \
  \( -size 1024x1024 -define gradient:direction=SouthEast gradient:'#7d6fff-#4536c4' \) \
  \( -size 1024x1024 xc:none -fill white -draw "roundrectangle 64,64 959,959 208,208" \) \
  -compose DstIn -composite -compose Over \
  -fill white -stroke none -draw "roundrectangle 212,300 812,680 80,80" -draw "polygon 352,670 482,670 352,790" \
  -fill none -stroke '#5747dc' -strokewidth 58 \
  -draw "stroke-linecap round line 436,414 588,566" -draw "stroke-linecap round line 588,414 436,566" \
  PNG32:assets/icon.png
npm run tauri icon assets/icon.png
rm -rf src-tauri/icons/android src-tauri/icons/ios
