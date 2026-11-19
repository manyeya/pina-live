//! What the installer does, its window aside: where Pina Live goes and what's there already,
//! putting it there (the old one swapped out whole, never half-replaced), and taking it out
//! again. Each step is a plain function, so the window can run them one after another off its
//! thread and say how each went.

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

/// The app, as it's named in Applications.
pub const APP: &str = "Pina Live.app";
/// Its program, inside it.
const PROGRAM: &str = "Contents/MacOS/pina-live";

/// Where it comes from and goes.
#[derive(Clone, Debug)]
pub struct Plan {
    /// The Pina Live.app to install: the installer's own.
    pub payload: PathBuf,
    /// Where apps go: /Applications, or ~/Applications if that can't be written to.
    pub apps: PathBuf,
}

pub(crate) fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

impl Plan {
    /// The installer's own: what it carries (beside it in its bundle, or named by
    /// `PINA_LIVE_INSTALL_PAYLOAD`) and where it puts it (`PINA_LIVE_INSTALL_APPS` says
    /// elsewhere, to try it out without touching this Mac's).
    pub fn here() -> Plan {
        let var = |k: &str| std::env::var_os(k).map(PathBuf::from);
        let resources = std::env::current_exe()
            .ok()
            .and_then(|e| Some(e.parent()?.parent()?.join("Resources")))
            .unwrap_or_default();
        let payload = var("PINA_LIVE_INSTALL_PAYLOAD").unwrap_or_else(|| resources.join(APP));
        let apps = var("PINA_LIVE_INSTALL_APPS").unwrap_or_else(|| {
            let system = PathBuf::from("/Applications");
            match writable(&system) {
                true => system,
                false => home().join("Applications"),
            }
        });
        Plan { payload, apps }
    }

    /// Where Pina Live is (or will be) installed.
    pub fn app(&self) -> PathBuf {
        self.apps.join(APP)
    }
}

/// Whether a folder can be written to: a file made in it and taken away again.
fn writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".pina-live-installer-{}", std::process::id()));
    match fs::create_dir(&probe) {
        Ok(()) => {
            let _ = fs::remove_dir(&probe);
            true
        }
        Err(_) => false,
    }
}

/// An app's version, from its Info.plist.
pub fn version(app: &Path) -> Option<String> {
    let plist = fs::read_to_string(app.join("Contents/Info.plist")).ok()?;
    let at = plist.find("<key>CFBundleShortVersionString</key>")?;
    let rest = &plist[at..];
    let start = rest.find("<string>")? + "<string>".len();
    let end = rest[start..].find("</string>")?;
    Some(rest[start..start + end].trim().to_string())
}

/// What installing does to what's there: nothing there, an older one, the same, or a newer.
#[derive(Clone, Debug, PartialEq)]
pub enum Over {
    Nothing,
    Older(String),
    Same,
    Newer(String),
}

/// Versions compared number by number ("0.10.2" after "0.9.7"); a suffix ("-dev") comes before
/// the release it's for.
fn numbers(v: &str) -> (Vec<u64>, bool) {
    let (main, suffix) = v.split_once('-').map_or((v, false), |(m, _)| (m, true));
    let n = main.split('.').map(|p| p.parse().unwrap_or(0)).collect();
    (n, !suffix)
}

pub fn over(installed: Option<String>, new: &str) -> Over {
    let Some(old) = installed else {
        return Over::Nothing;
    };
    match numbers(&old).cmp(&numbers(new)) {
        std::cmp::Ordering::Less => Over::Older(old),
        std::cmp::Ordering::Equal => Over::Same,
        std::cmp::Ordering::Greater => Over::Newer(old),
    }
}

/// Whether Pina Live is open from this app.
pub fn running(app: &Path) -> bool {
    let exe = app.join(PROGRAM);
    let exe = exe.to_string_lossy();
    let Ok(out) = Command::new("ps").args(["-axo", "command="]).output() else {
        return false;
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .any(|l| l.trim() == exe)
}

/// Puts Pina Live where it goes: copied beside the old one, then swapped in, so one is there
/// whole at every moment. Not marked as downloaded: the installer was the one opened.
pub fn copy_app(plan: &Plan) -> Result<PathBuf, String> {
    fs::create_dir_all(&plan.apps)
        .map_err(|e| format!("Couldn't make {}: {e}", plan.apps.display()))?;
    let dest = plan.app();
    let staging = plan.apps.join(format!(".{APP}.installing"));
    let old = plan.apps.join(format!(".{APP}.old"));
    let _ = fs::remove_dir_all(&staging);
    let _ = fs::remove_dir_all(&old);
    let copied = Command::new("ditto")
        .arg("--noqtn")
        .arg(&plan.payload)
        .arg(&staging)
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| e.to_string())?;
    if !copied.status.success() {
        let _ = fs::remove_dir_all(&staging);
        return Err(format!(
            "Couldn't copy Pina Live into {}: {}",
            plan.apps.display(),
            String::from_utf8_lossy(&copied.stderr).trim()
        ));
    }
    if dest.exists() {
        fs::rename(&dest, &old)
            .map_err(|e| format!("Couldn't move the old Pina Live aside: {e}"))?;
    }
    if let Err(e) = fs::rename(&staging, &dest) {
        // The old one back, as it was.
        let _ = fs::rename(&old, &dest);
        return Err(format!("Couldn't put Pina Live in place: {e}"));
    }
    let _ = fs::remove_dir_all(&old);
    Ok(dest)
}

/// Tells Launch Services about it: the Finder, Spotlight and the Dock see the new one now.
pub fn register(app: &Path) {
    let tool = "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister";
    let _ = Command::new(tool).arg("-f").arg(app).status();
}

/// A path as one word for the shell.
fn quoted(p: &Path) -> String {
    format!("'{}'", p.to_string_lossy().replace('\'', "'\\''"))
}

/// To the Trash, as the Finder sends things (tests take it away instead: they never touch this
/// Mac's Trash).
fn trash(p: &Path) -> Result<(), String> {
    if cfg!(test) || cfg!(not(target_os = "macos")) {
        return fs::remove_dir_all(p).map_err(|e| e.to_string());
    }
    #[cfg(target_os = "macos")]
    use objc2_foundation::{NSFileManager, NSString, NSURL};
    #[cfg(target_os = "macos")]
    {
        let url = NSURL::fileURLWithPath(&NSString::from_str(&p.to_string_lossy()));
        NSFileManager::defaultManager()
            .trashItemAtURL_resultingItemURL_error(&url, None)
            .map_err(|e| e.localizedDescription().to_string())
    }
    #[cfg(not(target_os = "macos"))]
    Ok(())
}

/// Pina Live's own settings, library and caches (never the music it plays).
pub fn settings() -> Vec<PathBuf> {
    let lib = home().join("Library");
    vec![
        lib.join("Application Support").join("Pina Live"),
        lib.join("Caches").join("Pina Live"),
    ]
}

/// Takes Pina Live out: the app to the Trash and (if asked) its settings, library and caches.
/// Music stays.
pub fn uninstall(plan: &Plan, and_settings: bool) -> Result<(), String> {
    let app = plan.app();
    if app.exists() {
        trash(&app).map_err(|e| format!("Couldn't move Pina Live to the Trash: {e}"))?;
    }
    if and_settings {
        for p in settings()
            .iter()
            .filter(|p| fs::symlink_metadata(p).is_ok())
        {
            trash(p)?;
        }
    }
    Ok(())
}

/// Opens Pina Live.
pub fn launch(app: &Path) {
    let _ = Command::new("open").arg(app).status();
}

/// The disk image the installer came on, put away once it's gone.
pub fn eject_later() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let mut parts = exe.components().skip(1);
    let (Some(first), Some(volume)) = (parts.next(), parts.next()) else {
        return;
    };
    if first.as_os_str() != "Volumes" {
        return;
    }
    let volume = Path::new("/Volumes").join(volume.as_os_str());
    let _ = Command::new("sh")
        .arg("-c")
        .arg(format!(
            "sleep 2; hdiutil detach {} -quiet",
            quoted(&volume)
        ))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

#[cfg(test)]
mod tests;
