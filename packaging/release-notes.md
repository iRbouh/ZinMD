## Downloads

| Your computer | To install | To run without installing |
| --- | --- | --- |
| Mac with Apple Silicon (M1 or later) | `ZinMD-{version}-macos-arm64.dmg` | `ZinMD-{version}-macos-arm64.zip` |
| Mac with an Intel processor | `ZinMD-{version}-macos-x64.dmg` | `ZinMD-{version}-macos-x64.zip` |
| Windows 10 or 11 | `ZinMD-{version}-windows-x64-setup.exe` | `ZinMD-{version}-windows-x64.zip` |
| Ubuntu, Debian, Mint, Pop!_OS | `ZinMD-{version}-linux-x64.deb` | `ZinMD-{version}-linux-x64.AppImage` |
| Fedora, openSUSE, Red Hat | `ZinMD-{version}-linux-x64.rpm` | `ZinMD-{version}-linux-x64.AppImage` |
| Any other Linux | `ZinMD-{version}-linux-x64.tar.gz` (unpack, then `./install.sh`) | `ZinMD-{version}-linux-x64.AppImage` |

The installed versions offer ZinMD for Markdown files: a `.md` file can be opened with it from the file manager. The versions that run without installing leave the system as it is.

`SHA256SUMS.txt` lists what each file should be, to check a download against.

## The first time you open it

ZinMD is not signed yet, so macOS and Windows ask before they open it.

- **macOS:** open ZinMD once and close the warning. Then go to System Settings, Privacy & Security, and choose **Open Anyway**. Or run `xattr -dr com.apple.quarantine /Applications/ZinMD.app` in Terminal.
- **Windows:** on the blue "Windows protected your PC" window choose **More info**, then **Run anyway**.
- **Linux AppImage:** make the file runnable first: `chmod +x ZinMD-*.AppImage`.
