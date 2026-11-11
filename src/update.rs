//! Keeping ZinMD up to date.
//!
//! Each release leaves a small file beside its downloads that says which
//! version it is and where the download for each kind of computer is. The
//! app reads it now and then. When it names a newer version, the right
//! download is fetched in the background and checked against what the file
//! says it should be. All that is left for the user is to say when: one
//! click swaps the new copy in and starts it.
//!
//! How the swap is made depends on how this copy got onto the machine:
//!
//! - macOS: the app bundle is replaced by the new one.
//! - Windows, installed: the new installer is run, without its windows.
//! - Windows from the zip, Linux AppImage, Linux unpacked from the .tar.gz:
//!   the one file that is the program is replaced.
//! - A Linux system package, or a copy somewhere that cannot be written to:
//!   nothing is swapped. The app says a new version is out and where to get it.
//!
//! `ZINMD_UPDATE_FEED` names another file to read, on the web or on disk:
//! that is how the release pipeline tries all this without a real release.
//! `ZINMD_UPDATE=now` restarts into the new version as soon as it is ready,
//! with nobody there to click; `ZINMD_UPDATE=off` never looks.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::system::{self, VERSION};

/// Where the newest release says what it is.
const FEED: &str = "https://github.com/iRbouh/ZinMD/releases/latest/download/latest.json";
/// Where every release is listed.
pub const RELEASES: &str = "https://github.com/iRbouh/ZinMD/releases";
/// Where a person goes to get the newest one by hand.
pub const DOWNLOADS: &str = "https://github.com/iRbouh/ZinMD/releases/latest";

/// How often a running app looks again.
pub const EVERY: Duration = Duration::from_secs(6 * 60 * 60);
/// How long after starting it first looks: well clear of opening the window.
pub const FIRST_LOOK: Duration = Duration::from_secs(4);

/// How this copy of ZinMD got onto the machine, which decides how a new
/// version takes its place.
#[derive(Clone, Debug, PartialEq)]
pub enum Install {
    /// macOS: an app bundle, in a folder that can be written to.
    Bundle(PathBuf),
    /// Windows: put in place by the installer. The path is the program's.
    Installer(PathBuf),
    /// One file that is the whole program, and can be written over: from the
    /// Windows zip, the Linux AppImage, or the Linux .tar.gz.
    Program(PathBuf),
    /// Somewhere a new copy cannot be put by us: a Linux system package, a
    /// read-only disk, another person's folder.
    Fixed,
    /// A developer's build, which is never updated.
    Developer,
}

impl Install {
    /// Work out how the running program was installed.
    pub fn detect() -> Self {
        let Ok(program) = std::env::current_exe() else {
            return Install::Fixed;
        };
        // Where the program really is, not a shortcut to it. (Windows gives
        // the real place already, and its long form of it confuses tools.)
        let program = match cfg!(target_os = "windows") {
            true => program,
            false => program.canonicalize().unwrap_or(program),
        };
        // A build made from the source tree stays what it is, unless a
        // rehearsal says otherwise.
        let rehearsal = std::env::var_os("ZINMD_UPDATE_FEED").is_some();
        let from_source = cfg!(debug_assertions)
            || program
                .components()
                .any(|part| part.as_os_str() == "target");
        if from_source && !rehearsal {
            return Install::Developer;
        }
        Self::of(&program)
    }

    fn of(program: &Path) -> Self {
        let folder = program.parent().unwrap_or(Path::new(""));
        if cfg!(target_os = "macos") {
            // ZinMD.app/Contents/MacOS/ZinMD
            let Some(bundle) = program.ancestors().nth(3) else {
                return Install::Developer;
            };
            if bundle
                .extension()
                .is_none_or(|extension| extension != "app")
            {
                return Install::Developer;
            }
            // An app opened straight from a download is run by macOS from a
            // hidden read-only copy; the real one cannot be found from here.
            let moved_aside = bundle
                .components()
                .any(|part| part.as_os_str() == "AppTranslocation");
            match bundle.parent() {
                Some(home) if !moved_aside && writable(home) => {
                    Install::Bundle(bundle.to_path_buf())
                }
                _ => Install::Fixed,
            }
        } else if cfg!(target_os = "windows") {
            if folder.join("unins000.exe").exists() {
                Install::Installer(program.to_path_buf())
            } else if writable(folder) {
                Install::Program(program.to_path_buf())
            } else {
                Install::Fixed
            }
        } else {
            // An AppImage runs from inside itself; the file to replace is
            // the AppImage, which it names.
            let appimage = std::env::var_os("APPIMAGE").map(PathBuf::from);
            let file = appimage.as_deref().unwrap_or(program);
            match file.parent() {
                Some(home) if file.is_file() && writable(home) => {
                    Install::Program(file.to_path_buf())
                }
                _ => Install::Fixed,
            }
        }
    }

    /// The name, in a release's list of downloads, of the one that updates
    /// this copy.
    fn download(&self) -> Option<String> {
        let processor = if cfg!(target_arch = "aarch64") {
            "arm64"
        } else {
            "x64"
        };
        let kind = match self {
            Install::Bundle(_) => "macos",
            Install::Installer(_) => "windows-setup",
            Install::Program(_) if cfg!(target_os = "windows") => "windows-zip",
            Install::Program(_) if std::env::var_os("APPIMAGE").is_some() => "linux-appimage",
            Install::Program(_) => "linux-tar",
            Install::Fixed | Install::Developer => return None,
        };
        Some(format!("{kind}-{processor}"))
    }
}

/// Whether files can be made in `folder`: by making one.
fn writable(folder: &Path) -> bool {
    let probe = folder.join(format!(".zinmd-{}", std::process::id()));
    let made = std::fs::File::create(&probe).is_ok();
    std::fs::remove_file(&probe).ok();
    made
}

/// A release newer than the running one.
#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub version: String,
    /// The page that tells what is new, and has every download.
    pub page: String,
    /// The download that updates this copy, if this copy can be updated.
    file: Option<Download>,
}

#[derive(Clone, Debug, PartialEq)]
struct Download {
    /// A web address, or a path on this machine.
    from: String,
    name: String,
    sha256: String,
    size: u64,
}

impl Release {
    /// How much there is to fetch.
    pub fn size(&self) -> u64 {
        self.file.as_ref().map_or(0, |file| file.size)
    }

    /// Whether this copy can take the new version in by itself.
    pub fn fetchable(&self) -> bool {
        self.file.is_some()
    }
}

#[cfg(feature = "shot")]
impl Release {
    /// A release that does not exist, for pictures of the card.
    pub fn pretend(version: &str, size: u64) -> Self {
        Release {
            version: version.into(),
            page: DOWNLOADS.into(),
            file: Some(Download {
                from: String::new(),
                name: String::new(),
                sha256: String::new(),
                size,
            }),
        }
    }
}

#[cfg(feature = "shot")]
impl Ready {
    /// An update that does not exist, for pictures of the card.
    pub fn pretend(version: &str) -> Self {
        Ready {
            version: version.into(),
            new: PathBuf::new(),
        }
    }
}

/// Whether looking for updates is turned off from outside.
pub fn silenced() -> bool {
    std::env::var("ZINMD_UPDATE").as_deref() == Ok("off")
}

/// Whether a ready update is to be started with nobody there to click.
pub fn unattended() -> bool {
    std::env::var("ZINMD_UPDATE").as_deref() == Ok("now")
}

/// Ask whether there is a newer release. `Ok(None)` when there is not.
/// Reaches the network: call from a background thread.
pub fn look(install: &Install) -> Result<Option<Release>, String> {
    let feed = std::env::var("ZINMD_UPDATE_FEED").unwrap_or_else(|_| FEED.to_string());
    let text = match read_text(&feed) {
        Ok(text) => text,
        // Before the first full release there is nothing to find there.
        Err(Problem::Missing) => return Ok(None),
        Err(Problem::Other(problem)) => return Err(problem),
    };
    newer_in(&text, install)
}

/// The release described by `text`, if it is newer than the running one.
fn newer_in(text: &str, install: &Install) -> Result<Option<Release>, String> {
    let described: Value = serde_json::from_str(text)
        .map_err(|problem| format!("the release list is unreadable: {problem}"))?;
    let version = described["version"]
        .as_str()
        .ok_or("the release list names no version")?;
    if !newer(version, VERSION) {
        return Ok(None);
    }
    let file = install.download().and_then(|name| {
        let file = &described["files"][&name];
        Some(Download {
            from: file["url"].as_str()?.to_string(),
            name: file["name"].as_str()?.to_string(),
            sha256: file["sha256"].as_str()?.to_lowercase(),
            size: file["size"].as_u64().unwrap_or(0),
        })
    });
    Ok(Some(Release {
        version: version.to_string(),
        page: described["page"].as_str().unwrap_or(DOWNLOADS).to_string(),
        file,
    }))
}

/// Whether `candidate` is a later version than `current`, as versions are
/// ordered: 0.2.0 after 0.1.9, 0.2.0 after 0.2.0-rc.2, rc.2 after rc.1.
pub fn newer(candidate: &str, current: &str) -> bool {
    order(candidate) > order(current)
}

/// One piece of what follows the dash in `0.2.0-rc.1`. Numbers come before words.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Piece {
    Number(u64),
    Word(String),
}

/// A version as something that can be compared: its three numbers, whether it
/// is a finished release (which comes after its own previews), and the
/// pieces of the preview's name.
fn order(version: &str) -> ([u64; 3], bool, Vec<Piece>) {
    let version = version.trim().trim_start_matches('v');
    let (numbers, preview) = match version.split_once('-') {
        Some((numbers, preview)) => (numbers, Some(preview)),
        None => (version, None),
    };
    let mut triple = [0; 3];
    for (slot, number) in triple.iter_mut().zip(numbers.split('.')) {
        *slot = number.parse().unwrap_or(0);
    }
    let pieces = preview
        .map(|preview| {
            preview
                .split('.')
                .map(|piece| match piece.parse() {
                    Ok(number) => Piece::Number(number),
                    Err(_) => Piece::Word(piece.to_string()),
                })
                .collect()
        })
        .unwrap_or_default();
    (triple, preview.is_none(), pieces)
}

/// An update fetched, checked and unpacked, waiting to be swapped in.
#[derive(Clone, Debug, PartialEq)]
pub struct Ready {
    pub version: String,
    /// The new app bundle, installer, or program file.
    new: PathBuf,
}

/// Where fetched updates are kept until they are used.
fn shelf() -> PathBuf {
    system::data_dir().join("update")
}

/// Fetch the download of `release`, check it, and unpack it. `fetched`
/// counts the bytes as they arrive. Slow: call from a background thread.
pub fn fetch(release: &Release, install: &Install, fetched: &AtomicU64) -> Result<Ready, String> {
    let file = release
        .file
        .as_ref()
        .ok_or("this copy of ZinMD cannot update itself")?;
    // One folder per version; what is left of older ones is cleared out.
    let folder = shelf().join(&release.version);
    if let Ok(others) = std::fs::read_dir(shelf()) {
        for other in others.flatten().filter(|other| other.path() != folder) {
            remove(&other.path());
        }
    }
    std::fs::create_dir_all(&folder)
        .map_err(|problem| format!("no room for the update: {problem}"))?;
    let packed = folder.join(&file.name);

    // A download left whole by an earlier run is not fetched again.
    if sha256_of(&packed).as_deref() != Ok(file.sha256.as_str()) {
        download(&file.from, &packed, fetched)?;
        let found = sha256_of(&packed)?;
        if found != file.sha256 {
            remove(&packed);
            return Err("the download is not the file the release says it is".into());
        }
    }
    fetched.store(file.size, Ordering::Relaxed);

    let unpacked = folder.join("unpacked");
    remove(&unpacked);
    let new = match install {
        Install::Bundle(_) => {
            // ditto is the tool macOS itself unzips apps with: it keeps the
            // links and the seal inside the bundle as they were.
            run(Command::new("ditto")
                .arg("-x")
                .arg("-k")
                .arg(&packed)
                .arg(&unpacked))?;
            unpacked.join("ZinMD.app")
        }
        Install::Installer(_) => packed,
        Install::Program(_) if packed.extension().is_some_and(|kind| kind == "AppImage") => packed,
        Install::Program(_) => {
            std::fs::create_dir_all(&unpacked).map_err(|problem| problem.to_string())?;
            run(Command::new("tar")
                .arg("-xf")
                .arg(&packed)
                .arg("-C")
                .arg(&unpacked))?;
            // ZinMD/ZinMD.exe in the zip, <name>/bin/zinmd in the .tar.gz.
            let inside = std::fs::read_dir(&unpacked)
                .ok()
                .and_then(|mut entries| entries.next()?.ok())
                .map(|entry| entry.path())
                .ok_or("the download is empty")?;
            match cfg!(target_os = "windows") {
                true => inside.join("ZinMD.exe"),
                false => inside.join("bin").join("zinmd"),
            }
        }
        Install::Fixed | Install::Developer => {
            return Err("this copy of ZinMD cannot update itself".into());
        }
    };
    let whole = match install {
        Install::Bundle(_) => new.join("Contents/MacOS/ZinMD").is_file(),
        _ => new.is_file(),
    };
    if !whole {
        return Err("the download does not hold ZinMD".into());
    }
    Ok(Ready {
        version: release.version.clone(),
        new,
    })
}

/// Put the new version where the running one is, and arrange for it to start
/// once this one has gone. The caller quits straight after. If this fails,
/// the running copy is as it was.
pub fn swap_in(ready: &Ready, install: &Install) -> Result<(), String> {
    let me = std::process::id();
    let mut start = match install {
        Install::Bundle(bundle) => {
            let home = bundle.parent().ok_or("the app is nowhere")?;
            let (incoming, outgoing) = (
                home.join(format!(".ZinMD-new-{me}.app")),
                home.join(format!(".ZinMD-old-{me}.app")),
            );
            // Beside the app first, so that taking its place is one rename.
            if std::fs::rename(&ready.new, &incoming).is_err() {
                run(Command::new("ditto").arg(&ready.new).arg(&incoming))?;
            }
            replace(bundle, &incoming, &outgoing)?;
            // Once this program has gone: clear the old app away, open the new.
            let mut shell = Command::new("/bin/sh");
            shell
                .arg("-c")
                .arg(r#"while kill -0 "$0" 2>/dev/null; do sleep 0.1; done; rm -rf "$1"; shift; exec open -n "$@""#)
                .arg(me.to_string())
                .arg(&outgoing);
            // An app opened this way starts with a clean slate; what this
            // copy was told about where to keep things goes with it by hand.
            for name in ["ZINMD_DATA", "ZINMD_UPDATE_FEED", "ZINMD_TRACE"] {
                if let Ok(value) = std::env::var(name) {
                    shell.arg("--env").arg(format!("{name}={value}"));
                }
            }
            shell.arg(bundle);
            shell
        }
        Install::Installer(_) => {
            // The installer waits for this program to go, puts the new one
            // in its place, and starts it.
            let mut installer = Command::new(&ready.new);
            installer.args([
                "/VERYSILENT",
                "/SUPPRESSMSGBOXES",
                "/NORESTART",
                "/UPDATE=1",
            ]);
            installer
        }
        Install::Program(program) => {
            let home = program.parent().ok_or("the program is nowhere")?;
            let (incoming, outgoing) = (
                home.join(format!(".zinmd-new-{me}")),
                // Windows will not delete a program that is running, but lets
                // it be renamed. It is cleared away at the next start.
                aside(program),
            );
            std::fs::copy(&ready.new, &incoming).map_err(|problem| {
                format!("the new version could not be put in place: {problem}")
            })?;
            make_runnable(&incoming);
            if cfg!(target_os = "windows") {
                replace(program, &incoming, &outgoing)?;
                Command::new(program)
            } else {
                // Elsewhere the new file simply takes the name; the running
                // program keeps the old one until it ends.
                std::fs::rename(&incoming, program).map_err(|problem| {
                    remove(&incoming);
                    format!("the new version could not be put in place: {problem}")
                })?;
                // Once this program has gone, start the new one.
                let mut shell = Command::new("/bin/sh");
                shell
                    .arg("-c")
                    .arg(r#"while kill -0 "$0" 2>/dev/null; do sleep 0.1; done; exec "$1""#)
                    .arg(me.to_string())
                    .arg(program);
                shell
            }
        }
        Install::Fixed | Install::Developer => {
            return Err("this copy of ZinMD cannot update itself".into());
        }
    };

    // The new copy says "updated" when it finds this note at its first start.
    std::fs::write(shelf().join("updated"), VERSION).ok();
    start
        .env_remove("ZINMD_UPDATE")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    detach(&mut start);
    start
        .spawn()
        .map(drop)
        .map_err(|problem| format!("the new version could not be started: {problem}"))
}

/// Put `incoming` where `current` is, moving `current` to `outgoing`. If the
/// second step fails the first is undone.
fn replace(current: &Path, incoming: &Path, outgoing: &Path) -> Result<(), String> {
    remove(outgoing);
    std::fs::rename(current, outgoing)
        .map_err(|problem| format!("the running version could not be moved aside: {problem}"))?;
    std::fs::rename(incoming, current).map_err(|problem| {
        std::fs::rename(outgoing, current).ok();
        remove(incoming);
        format!("the new version could not be put in place: {problem}")
    })
}

/// Where a replaced Windows program waits to be deleted.
fn aside(program: &Path) -> PathBuf {
    let mut name = program.file_name().unwrap_or_default().to_os_string();
    name.push(".old");
    program.with_file_name(name)
}

/// Let a command outlive this program, and whatever started this program.
fn detach(command: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        // Its own group, and no console of ours to hold on to.
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    #[cfg(not(any(unix, windows)))]
    let _ = command;
}

fn make_runnable(file: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o755)).ok();
    }
    #[cfg(not(unix))]
    let _ = file;
}

/// At the start of a run: clear away what an update left behind, and say
/// which version this copy replaced if it has just done so.
pub fn settle() -> Option<String> {
    if let Ok(program) = std::env::current_exe() {
        remove(&aside(&program));
    }
    let note = shelf().join("updated");
    let before = std::fs::read_to_string(&note).ok()?;
    // The update did what it was for: nothing on the shelf is needed again.
    remove(&shelf());
    (before.trim() != VERSION).then(|| before.trim().to_string())
}

/// Delete a file or a folder, whichever it is, if it is there.
fn remove(path: &Path) {
    if path.is_dir() {
        std::fs::remove_dir_all(path).ok();
    } else {
        std::fs::remove_file(path).ok();
    }
}

/// Run a system tool to its end, quietly.
fn run(command: &mut Command) -> Result<(), String> {
    let name = command.get_program().to_string_lossy().into_owned();
    hide(command);
    let output = command
        .stdin(Stdio::null())
        .output()
        .map_err(|problem| format!("{name} could not be run: {problem}"))?;
    match output.status.success() {
        true => Ok(()),
        false => Err(format!(
            "{name} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )),
    }
}

/// Keep a console window from flashing up behind a tool on Windows.
fn hide(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    let _ = command;
}

enum Problem {
    /// There is nothing at that address.
    Missing,
    Other(String),
}

/// What to do with an address: fetch it from the web, or read it off the disk.
enum Place<'a> {
    Web(&'a str),
    Disk(&'a Path),
}

fn place(address: &str) -> Result<Place<'_>, String> {
    if address.starts_with("https://") {
        Ok(Place::Web(address))
    } else if address.starts_with("http://") {
        // What is fetched is run: it comes over a checked connection or not at all.
        Err("updates are only fetched over https".into())
    } else {
        Ok(Place::Disk(Path::new(
            address.strip_prefix("file://").unwrap_or(address),
        )))
    }
}

fn web() -> ureq::Agent {
    ureq::Agent::config_builder()
        .user_agent(format!("ZinMD/{VERSION}"))
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_recv_response(Some(Duration::from_secs(30)))
        // A stalled download is given up on; a slow one is not.
        .timeout_per_read(Some(Duration::from_secs(30)))
        .build()
        .new_agent()
}

fn read_text(address: &str) -> Result<String, Problem> {
    match place(address).map_err(Problem::Other)? {
        Place::Web(address) => match web().get(address).call() {
            Ok(mut answer) => answer
                .body_mut()
                .read_to_string()
                .map_err(|problem| Problem::Other(problem.to_string())),
            Err(ureq::Error::StatusCode(404)) => Err(Problem::Missing),
            Err(problem) => Err(Problem::Other(problem.to_string())),
        },
        Place::Disk(path) => {
            std::fs::read_to_string(path).map_err(|problem| match problem.kind() {
                std::io::ErrorKind::NotFound => Problem::Missing,
                _ => Problem::Other(problem.to_string()),
            })
        }
    }
}

/// Copy what is at `address` to `to`, counting the bytes into `fetched`.
fn download(address: &str, to: &Path, fetched: &AtomicU64) -> Result<(), String> {
    let failed = |problem: &dyn std::fmt::Display| format!("the download failed: {problem}");
    let mut source: Box<dyn Read> = match place(address)? {
        Place::Web(address) => Box::new(
            web()
                .get(address)
                .call()
                .map_err(|problem| failed(&problem))?
                .into_body()
                .into_reader(),
        ),
        Place::Disk(path) => {
            Box::new(std::fs::File::open(path).map_err(|problem| failed(&problem))?)
        }
    };
    // Written under another name until it is whole.
    let partial = to.with_extension("part");
    let mut out = std::fs::File::create(&partial).map_err(|problem| failed(&problem))?;
    let mut buffer = vec![0; 64 * 1024];
    fetched.store(0, Ordering::Relaxed);
    loop {
        let read = source
            .read(&mut buffer)
            .map_err(|problem| failed(&problem))?;
        if read == 0 {
            break;
        }
        out.write_all(&buffer[..read])
            .map_err(|problem| failed(&problem))?;
        fetched.fetch_add(read as u64, Ordering::Relaxed);
    }
    out.sync_all().ok();
    drop(out);
    std::fs::rename(&partial, to).map_err(|problem| failed(&problem))
}

/// The SHA-256 of a file, in lower-case hex.
fn sha256_of(file: &Path) -> Result<String, String> {
    let mut file = std::fs::File::open(file).map_err(|problem| problem.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|problem| problem.to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

/// Bytes arriving, shared between the thread that fetches and the window
/// that shows how far it has got.
pub type Fetched = Arc<AtomicU64>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_ordered_as_people_expect() {
        for (later, earlier) in [
            ("0.2.0", "0.1.9"),
            ("0.10.0", "0.9.0"),
            ("1.0.0", "0.99.99"),
            ("0.2.0", "0.2.0-rc.2"),
            ("0.2.0-rc.2", "0.2.0-rc.1"),
            ("0.2.0-rc.10", "0.2.0-rc.9"),
            ("0.2.0-rc.1", "0.2.0-beta.4"),
            ("0.2.0-rc.1", "0.1.9"),
            ("v0.2.0", "0.1.0"),
        ] {
            assert!(newer(later, earlier), "{later} should follow {earlier}");
            assert!(
                !newer(earlier, later),
                "{earlier} should not follow {later}"
            );
        }
        assert!(!newer("0.2.0", "0.2.0"));
        assert!(!newer("0.2.0-rc.1", "0.2.0-rc.1"));
    }

    fn list(version: &str) -> String {
        serde_json::json!({
            "version": version,
            "page": "https://example.com/notes",
            "files": {
                "macos-arm64": { "name": "a.zip", "url": "https://example.com/a.zip", "sha256": "AB12", "size": 7 },
                "macos-x64": { "name": "b.zip", "url": "https://example.com/b.zip", "sha256": "cd34", "size": 8 },
            },
        })
        .to_string()
    }

    #[test]
    fn a_newer_release_is_found_with_the_download_for_this_copy() {
        let install = Install::Bundle("/Applications/ZinMD.app".into());
        let release = newer_in(&list("999.0.0"), &install).unwrap().unwrap();
        assert_eq!(release.version, "999.0.0");
        assert_eq!(release.page, "https://example.com/notes");
        if cfg!(target_os = "macos") {
            assert!(release.fetchable());
            let file = release.file.unwrap();
            // Checksums are compared in lower case, however they were written.
            assert!(file.sha256 == "ab12" || file.sha256 == "cd34");
        }
    }

    #[test]
    fn the_running_version_and_older_ones_are_not_updates() {
        let install = Install::Fixed;
        assert_eq!(newer_in(&list(VERSION), &install), Ok(None));
        assert_eq!(newer_in(&list("0.0.1"), &install), Ok(None));
        assert!(newer_in("not a list", &install).is_err());
    }

    #[test]
    fn a_copy_that_cannot_be_replaced_is_only_told_where_to_look() {
        let release = newer_in(&list("999.0.0"), &Install::Fixed)
            .unwrap()
            .unwrap();
        assert!(!release.fetchable());
        assert_eq!(release.size(), 0);
    }

    #[test]
    fn plain_http_is_refused() {
        assert!(place("http://example.com/latest.json").is_err());
        assert!(matches!(place("https://example.com/x"), Ok(Place::Web(_))));
        assert!(matches!(place("/tmp/latest.json"), Ok(Place::Disk(_))));
        assert!(matches!(
            place("file:///tmp/latest.json"),
            Ok(Place::Disk(_))
        ));
    }

    #[test]
    fn a_download_is_copied_whole_and_its_checksum_taken() {
        let folder = std::env::temp_dir().join(format!("zinmd-update-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let (from, to) = (folder.join("from"), folder.join("to"));
        std::fs::write(&from, b"abc").unwrap();
        let fetched = AtomicU64::new(0);
        download(from.to_str().unwrap(), &to, &fetched).unwrap();
        assert_eq!(fetched.load(Ordering::Relaxed), 3);
        assert_eq!(
            sha256_of(&to).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn one_file_takes_the_place_of_another_or_nothing_changes() {
        let folder = std::env::temp_dir().join(format!("zinmd-swap-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let (current, incoming, outgoing) =
            (folder.join("app"), folder.join("new"), folder.join("old"));
        std::fs::write(&current, "one").unwrap();
        std::fs::write(&incoming, "two").unwrap();
        replace(&current, &incoming, &outgoing).unwrap();
        assert_eq!(std::fs::read_to_string(&current).unwrap(), "two");
        assert_eq!(std::fs::read_to_string(&outgoing).unwrap(), "one");

        // With no new file to put in place, the current one stays.
        assert!(replace(&current, &incoming, &outgoing).is_err());
        assert_eq!(std::fs::read_to_string(&current).unwrap(), "two");
        std::fs::remove_dir_all(&folder).ok();
    }
}
