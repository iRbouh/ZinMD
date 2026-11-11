#!/bin/sh
# tex.sh in.png out.png width height radius : resize, round the corners, and shear onto the left face of an isometric box
in="$1"; out="$2"; w="$3"; h="$4"; r="$5"
magick "$in" -resize "${w}x${h}!" \
  \( -size "${w}x${h}" xc:none -fill white -draw "roundrectangle 0,0 $((w-1)),$((h-1)) $r,$r" \) \
  -alpha set -compose DstIn -composite \
  -background none -virtual-pixel transparent +distort AffineProjection "0.8660254,0.5,0,1,0,0" +repage "$out"
