//! Tells you when a session needs you: a system notification and Lulo's
//! own sound. Both run on a worker thread so the widget never waits on them.
//!
//! An app without an MSIX package can't give its toasts a custom sound, so
//! the toast stays silent and the sound plays from memory with PlaySound.

use std::sync::mpsc::{self, Sender};
use std::time::{Duration, Instant};

use crate::changes::{Event, Kind};

/// Never more than one sound this often, however many sessions change.
const SOUND_GAP: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    pub toasts: bool,
    pub sounds: bool,
    pub new_session_sound: bool,
}

pub struct Alerts {
    tx: Option<Sender<(Vec<Event>, Options)>>,
}

impl Alerts {
    pub fn start() -> Self {
        let (tx, rx) = mpsc::channel::<(Vec<Event>, Options)>();
        let spawned = std::thread::Builder::new()
            .name("lulo-alerts".into())
            .spawn(move || {
                imp::init();
                let mut last_sound: Option<Instant> = None;
                for (events, options) in rx {
                    let quiet = imp::quiet();
                    if options.sounds && !quiet {
                        let kind = events
                            .iter()
                            .map(|e| e.kind)
                            .find(|k| *k != Kind::New || options.new_session_sound);
                        let due = last_sound.is_none_or(|t| t.elapsed() >= SOUND_GAP);
                        if let (Some(kind), true) = (kind, due) {
                            imp::play(kind);
                            last_sound = Some(Instant::now());
                        }
                    }
                    if options.toasts {
                        for e in events.iter().filter(|e| e.kind != Kind::New) {
                            imp::toast(&e.title(), e.detail.as_deref().unwrap_or(""));
                        }
                    }
                }
            });
        Alerts {
            tx: spawned.ok().map(|_| tx),
        }
    }

    pub fn send(&self, events: Vec<Event>, options: Options) {
        if events.is_empty() || !(options.toasts || options.sounds) {
            return;
        }
        if let Some(tx) = &self.tx {
            let _ = tx.send((events, options));
        }
    }
}

/// The sounds Francisco picked: Burbujas, Plop, Glub and Burbuja.
fn sound(kind: Kind) -> &'static [u8] {
    match kind {
        Kind::Waiting => include_bytes!("../../../assets/sounds/esperando.wav"),
        Kind::Done => include_bytes!("../../../assets/sounds/termino.wav"),
        Kind::Error => include_bytes!("../../../assets/sounds/error.wav"),
        Kind::New => include_bytes!("../../../assets/sounds/nueva.wav"),
    }
}

/// Identifies Lulo's notifications to Windows. The installer gives the Start
/// menu shortcut the same id.
#[cfg(windows)]
pub const APP_ID: &str = "Lulo.Widget";

#[cfg(windows)]
mod imp {
    use windows::core::HSTRING;
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};
    use windows::UI::Notifications::{
        ToastNotification, ToastNotificationManager, ToastNotificationMode,
    };
    use windows_sys::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};

    use super::APP_ID;

    pub fn init() {
        unsafe {
            let _ = RoInitialize(RO_INIT_MULTITHREADED);
        }
    }

    /// Do not disturb ("No molestar"), or a game, video or presentation on screen.
    pub fn quiet() -> bool {
        let dnd = ToastNotificationManager::GetDefault()
            .and_then(|m| m.NotificationMode())
            .is_ok_and(|mode| mode != ToastNotificationMode::Unrestricted);
        dnd || crate::display::busy_fullscreen()
    }

    pub fn play(kind: super::Kind) {
        let wav = super::sound(kind);
        // SND_MEMORY reads the bytes while it plays; they are 'static.
        unsafe {
            PlaySoundW(
                wav.as_ptr().cast(),
                std::ptr::null_mut(),
                SND_MEMORY | SND_ASYNC | SND_NODEFAULT,
            );
        }
    }

    pub fn toast(title: &str, body: &str) {
        let _ = try_toast(title, body);
    }

    fn try_toast(title: &str, body: &str) -> windows::core::Result<()> {
        let xml = format!(
            r#"<toast><visual><binding template="ToastGeneric"><text>{}</text><text>{}</text></binding></visual><audio silent="true"/></toast>"#,
            escape(title),
            escape(body)
        );
        let doc = XmlDocument::new()?;
        doc.LoadXml(&HSTRING::from(xml))?;
        let toast = ToastNotification::CreateToastNotification(&doc)?;
        ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(APP_ID))?.Show(&toast)
    }

    fn escape(s: &str) -> String {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    }
}

/// Linux: `notify-send` for the notification and the first sound player
/// found (PipeWire, PulseAudio, ALSA) for the sound, which plays a copy of
/// the WAV kept in Lulo's data folder.
#[cfg(target_os = "linux")]
mod imp {
    use std::process::{Command, Stdio};

    use crate::changes::Kind;
    use crate::linux;

    const PLAYERS: [&str; 3] = ["pw-play", "paplay", "aplay"];

    pub fn init() {}

    /// Do not disturb in the notification daemon, or something full screen.
    pub fn quiet() -> bool {
        let says = |cmd: &str, args: &[&str], busy: &str| {
            linux::output(cmd, args).is_some_and(|out| out.trim().contains(busy))
        };
        says("swaync-client", &["-D"], "true")
            || says("dunstctl", &["is-paused"], "true")
            || says("makoctl", &["mode"], "do-not-disturb")
            || crate::display::busy_fullscreen()
    }

    pub fn play(kind: Kind) {
        let name = match kind {
            Kind::Waiting => "esperando.wav",
            Kind::Done => "termino.wav",
            Kind::Error => "error.wav",
            Kind::New => "nueva.wav",
        };
        let path = linux::data_file(&format!("sounds/{name}"), super::sound(kind));
        let Some(path) = path else {
            return;
        };
        // Its own thread, so a slow player never holds up the next alert.
        let _ = std::thread::Builder::new()
            .name("lulo-sound".into())
            .spawn(move || {
                for player in PLAYERS {
                    let played = Command::new(player)
                        .arg(&path)
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .status();
                    // Only a missing player moves on to the next one.
                    if played.is_ok() {
                        return;
                    }
                }
            });
    }

    pub fn toast(title: &str, body: &str) {
        let icon = linux::icon();
        let mut args = vec!["--app-name=Lulo", "--hint=string:desktop-entry:lulo"];
        let icon_arg = icon.map(|p| format!("--icon={}", p.display()));
        if let Some(i) = &icon_arg {
            args.push(i);
        }
        let (title, body) = (escape(title), escape(body));
        args.extend(["--", &title, &body]);
        linux::run("notify-send", &args);
    }

    /// Most daemons read the body as markup.
    fn escape(s: &str) -> String {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
mod imp {
    pub fn init() {}
    pub fn quiet() -> bool {
        false
    }
    pub fn play(_kind: crate::changes::Kind) {}
    pub fn toast(_title: &str, _body: &str) {}
}

/// Lets Windows show Lulo's toasts even without the installer's shortcut:
/// registers the app id with a name and icon, and claims it for this process.
#[cfg(windows)]
pub fn register() {
    use windows_sys::Win32::System::Registry::{RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ};
    use windows_sys::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;

    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let key = wide(&format!(r"Software\Classes\AppUserModelId\{APP_ID}"));
    let set = |name: &str, value: &str| {
        let (name, value) = (wide(name), wide(value));
        unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                REG_SZ,
                value.as_ptr().cast(),
                (value.len() * 2) as u32,
            );
        }
    };
    set("DisplayName", "Lulo");
    if let Some(icon) = icon_file() {
        set("IconUri", &icon.to_string_lossy());
    }
    unsafe {
        SetCurrentProcessExplicitAppUserModelID(wide(APP_ID).as_ptr());
    }
}

#[cfg(not(windows))]
pub fn register() {}

/// The icon toasts show, written next to the settings once.
#[cfg(windows)]
fn icon_file() -> Option<std::path::PathBuf> {
    let path = crate::settings::dir()?.join("lulo.ico");
    if !path.exists() {
        std::fs::create_dir_all(path.parent()?).ok()?;
        std::fs::write(&path, include_bytes!("../../../assets/lulo.ico")).ok()?;
    }
    Some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_sound_is_a_wav() {
        for kind in [Kind::Waiting, Kind::Done, Kind::Error, Kind::New] {
            let wav = sound(kind);
            assert_eq!((&wav[..4], &wav[8..12]), (&b"RIFF"[..], &b"WAVE"[..]));
        }
    }
}
