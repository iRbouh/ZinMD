#!/usr/bin/env python3
"""Write the small file a release leaves beside its downloads, which tells a
running ZinMD that a newer version is out and where to get it.

    scripts/release-list.py VERSION FOLDER [ADDRESS] > latest.json

FOLDER holds the downloads. ADDRESS is where they will be on the web, as in
https://github.com/iRbouh/ZinMD/releases/download/v0.2.0. Left out, the list
points at the files where they are on this machine: that is how the app is
tried against a release that has not been made.

Each kind of installed copy looks up one download in the list: the Mac app
takes the zip of the app, a Windows copy put in place by the installer takes
the installer, and so on. See src/update.rs.
"""
import hashlib
import json
import os
import re
import sys

# The end of a download's name, and what the app calls that kind of download.
KINDS = [
    (r"-macos-(arm64|x64)\.zip$", "macos"),
    (r"-windows-(arm64|x64)-setup\.exe$", "windows-setup"),
    (r"-windows-(arm64|x64)\.zip$", "windows-zip"),
    (r"-linux-(arm64|x64)\.AppImage$", "linux-appimage"),
    (r"-linux-(arm64|x64)\.tar\.gz$", "linux-tar"),
]


def sha256(path):
    digest = hashlib.sha256()
    with open(path, "rb") as file:
        for block in iter(lambda: file.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def main():
    if len(sys.argv) < 3:
        sys.exit(__doc__)
    version, folder = sys.argv[1], sys.argv[2]
    address = sys.argv[3].rstrip("/") if len(sys.argv) > 3 else None
    files = {}
    for name in sorted(os.listdir(folder)):
        for ending, kind in KINDS:
            found = re.search(ending, name)
            if not found:
                continue
            path = os.path.join(folder, name)
            files[f"{kind}-{found.group(1)}"] = {
                "name": name,
                "url": f"{address}/{name}" if address else os.path.abspath(path),
                "sha256": sha256(path),
                "size": os.path.getsize(path),
            }
    json.dump(
        {
            "version": version,
            "page": f"https://github.com/iRbouh/ZinMD/releases/tag/v{version}",
            "files": files,
        },
        sys.stdout,
        indent=2,
    )
    sys.stdout.write("\n")


if __name__ == "__main__":
    main()
