//! What Nova Island keeps an eye on, in the background: which apps use the microphone and
//! camera, the network (Wi-Fi or cable, and whether it reaches the internet), the battery,
//! and the speakers' volume and the screen's brightness. Changes come to the notch as events.
//! It runs only while it's switched on (Nova mode); off, it sleeps.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq)]
pub enum Link {
    None,
    Wifi,
    Ethernet,
}

#[derive(Debug)]
pub enum Event {
    /// The apps using the microphone and the camera now.
    Privacy { mic: Vec<String>, cam: Vec<String> },
    /// The link and whether it reaches the internet (None: not known yet), with the last.
    Network { link: Link, internet: Option<bool>, prev: (Link, Option<bool>) },
    /// On the charger or not, and the charge (0..1).
    Battery { charging: bool, level: f32 },
    /// The speakers' volume (0..1) and whether they're muted.
    Volume { level: f32, muted: bool },
    /// The microphone (Windows' default one) muted or not: its first reading too.
    MicMute(bool),
    /// The screen's brightness (0..100), on screens that have one in software.
    Brightness(u32),
}

pub struct Watch {
    pub events: Receiver<Event>,
    on: Arc<AtomicBool>,
}

impl Watch {
    pub fn start(on: bool) -> Self {
        let (tx, rx) = channel();
        let flag = Arc::new(AtomicBool::new(on));
        let f = flag.clone();
        std::thread::spawn(move || run(tx, f));
        Self { events: rx, on: flag }
    }

    pub fn set_on(&self, on: bool) {
        self.on.store(on, Ordering::Relaxed);
    }
}

fn run(tx: Sender<Event>, on: Arc<AtomicBool>) {
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
    }
    let mut privacy: Option<(Vec<String>, Vec<String>)> = None;
    let mut net: (Link, Option<bool>) = (Link::None, None);
    let mut net_at = Instant::now() - Duration::from_secs(60);
    let mut probe_at = Instant::now() - Duration::from_secs(60);
    let mut battery: Option<(bool, u8)> = None;
    let mut volume: Option<(f32, bool)> = None;
    let mut bright: Option<u32> = None;
    let mut bright_at = Instant::now() - Duration::from_secs(60);
    let mut privacy_at = Instant::now() - Duration::from_secs(60);
    let mut wmi = None;
    let mut audio = None;
    let mut mic: Option<windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume> = None;
    let mut mic_at = Instant::now() - Duration::from_secs(60);
    let mut mic_muted: Option<bool> = None;
    let mut was_on = false;
    loop {
        std::thread::sleep(Duration::from_millis(250));
        if !on.load(Ordering::Relaxed) {
            // (A fresh start when it's back: the first readings are not news.)
            was_on = false;
            continue;
        }
        if !was_on {
            was_on = true;
            privacy = None;
            net = (Link::None, None);
            battery = None;
            volume = None;
            bright = None;
            mic_muted = None;
        }
        let now = Instant::now();
        // The speakers, four times a second (a change shows at once).
        if audio.is_none() {
            audio = endpoint_volume();
        }
        if let Some(a) = &audio {
            match unsafe { (a.GetMasterVolumeLevelScalar(), a.GetMute()) } {
                (Ok(level), Ok(muted)) => {
                    let v = (level, muted.as_bool());
                    // (The first reading too: the music page's slider shows it.)
                    if volume.is_none_or(|old| (old.0 - v.0).abs() > 0.004 || old.1 != v.1) {
                        let _ = tx.send(Event::Volume { level: v.0, muted: v.1 });
                    }
                    volume = Some(v);
                }
                // The default device changed: look it up again.
                _ => audio = None,
            }
        }
        // The microphone's mute, four times a second (the default microphone looked up
        // again every 5 s, in case it changed).
        if now - mic_at >= Duration::from_secs(5) {
            mic_at = now;
            mic = mic_volume();
        }
        if let Some(m) = &mic {
            if let Ok(muted) = unsafe { m.GetMute() } {
                let muted = muted.as_bool();
                if mic_muted != Some(muted) {
                    mic_muted = Some(muted);
                    let _ = tx.send(Event::MicMute(muted));
                }
            }
        }
        // Microphone and camera, every 2 s.
        if now - privacy_at >= Duration::from_secs(2) {
            privacy_at = now;
            let p = (in_use("microphone"), in_use("webcam"));
            if privacy.as_ref() != Some(&p) {
                let _ = tx.send(Event::Privacy { mic: p.0.clone(), cam: p.1.clone() });
                privacy = Some(p);
            }
            if let Some(b) = battery_now() {
                if battery.is_some_and(|old| old != b) {
                    let _ = tx.send(Event::Battery { charging: b.0, level: b.1 as f32 / 100.0 });
                }
                battery = Some(b);
            }
        }
        // The screen's brightness, every 2 s (laptop panels).
        if now - bright_at >= Duration::from_secs(2) {
            bright_at = now;
            if wmi.is_none() {
                wmi = wmi_connect();
            }
            if let Some(b) = wmi.as_ref().and_then(brightness) {
                if bright.is_some_and(|old| old != b) {
                    let _ = tx.send(Event::Brightness(b));
                }
                bright = Some(b);
            }
        }
        // The network: the link every 5 s; the internet every 15 s (5 s while offline, to
        // notice its return quickly), and whenever the link changes.
        if now - net_at >= Duration::from_secs(5) {
            net_at = now;
            let link = link_kind();
            let mut internet = net.1;
            let offline = internet == Some(false) || link == Link::None;
            if link == Link::None {
                internet = Some(false);
            } else if now - probe_at > Duration::from_secs(if offline { 5 } else { 15 }) || link != net.0 {
                probe_at = now;
                internet = Some(probe());
            }
            if (link.clone(), internet) != net {
                let prev = std::mem::replace(&mut net, (link.clone(), internet));
                let _ = tx.send(Event::Network { link, internet, prev });
            }
        }
    }
}

/// The apps using a device now ("microphone" or "webcam"): Windows notes every app's use
/// under CapabilityAccessManager\ConsentStore, and one is using it while its
/// LastUsedTimeStop is 0.
fn in_use(device: &str) -> Vec<String> {
    use windows::core::HSTRING;
    use windows::Win32::System::Registry::*;
    let base = format!(r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\{device}");
    let mut apps = Vec::new();
    unsafe fn subkeys(k: HKEY) -> Vec<String> {
        let mut out = Vec::new();
        let mut i = 0;
        loop {
            let mut buf = [0u16; 512];
            let mut len = buf.len() as u32;
            if RegEnumKeyExW(k, i, windows::core::PWSTR(buf.as_mut_ptr()), &mut len, None, windows::core::PWSTR::null(), None, None).is_err() {
                break;
            }
            out.push(String::from_utf16_lossy(&buf[..len as usize]));
            i += 1;
        }
        out
    }
    unsafe fn stopped(k: HKEY, name: &str) -> Option<u64> {
        let mut v = 0u64;
        let mut size = 8u32;
        RegGetValueW(k, &HSTRING::from(name), &HSTRING::from("LastUsedTimeStop"), RRF_RT_REG_QWORD, None, Some(&mut v as *mut u64 as _), Some(&mut size)).ok().ok()?;
        Some(v)
    }
    unsafe {
        let mut root = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, &HSTRING::from(base.as_str()), 0, KEY_READ, &mut root).is_err() {
            return apps;
        }
        for name in subkeys(root) {
            if name == "NonPackaged" {
                let mut np = HKEY::default();
                if RegOpenKeyExW(root, &HSTRING::from("NonPackaged"), 0, KEY_READ, &mut np).is_ok() {
                    for exe in subkeys(np) {
                        if stopped(np, &exe) == Some(0) {
                            apps.push(app_name(&exe));
                        }
                    }
                    let _ = RegCloseKey(np);
                }
            } else if stopped(root, &name) == Some(0) {
                apps.push(app_name(&name));
            }
        }
        let _ = RegCloseKey(root);
    }
    apps.sort();
    apps.dedup();
    apps
}

/// "C:#Program Files#Google#Chrome#Application#chrome.exe" or
/// "Microsoft.WindowsCamera_8wekyb3d8bbwe": a name to show.
fn app_name(key: &str) -> String {
    let raw = if key.contains('#') {
        let leaf = key.rsplit('#').next().unwrap_or(key);
        leaf.strip_suffix(".exe").or_else(|| leaf.strip_suffix(".EXE")).unwrap_or(leaf).to_owned()
    } else {
        key.split('_').next().unwrap_or(key).rsplit('.').next().unwrap_or(key).to_owned()
    };
    const NAMES: [(&str, &str); 26] = [
        ("chrome", "Chrome"), ("msedge", "Edge"), ("firefox", "Firefox"), ("brave", "Brave"), ("opera", "Opera"),
        ("vivaldi", "Vivaldi"), ("teams", "Teams"), ("ms-teams", "Teams"), ("zoom", "Zoom"), ("discord", "Discord"),
        ("slack", "Slack"), ("skype", "Skype"), ("obs64", "OBS"), ("obs", "OBS"), ("telegram", "Telegram"),
        ("whatsapp", "WhatsApp"), ("windowscamera", "Camera"), ("windowssoundrecorder", "Sound Recorder"),
        ("soundrec", "Sound Recorder"), ("electron", "Electron"), ("screensketch", "Snipping Tool"),
        ("loom-recorder-production", "Loom"), ("rundll32", "Windows"), ("immersivecontrolpanel", "Settings"),
        ("adobe audition cc", "Adobe Audition"), ("claude", "Claude"),
    ];
    let low = raw.to_lowercase();
    NAMES.iter().find(|n| n.0 == low).map_or_else(
        || {
            let mut c = raw.chars();
            c.next().map_or(String::new(), |f| f.to_uppercase().collect::<String>() + c.as_str())
        },
        |n| n.1.to_owned(),
    )
}

/// Wi-Fi, cable or nothing: any adapter that's up with a real IPv4 address (Wi-Fi wins).
fn link_kind() -> Link {
    use windows::Win32::NetworkManagement::IpHelper::*;
    use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
    use windows::Win32::Networking::WinSock::{AF_INET, SOCKADDR_IN};
    let mut size = 16 * 1024u32;
    let mut buf = vec![0u8; size as usize];
    let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;
    unsafe {
        if GetAdaptersAddresses(AF_INET.0 as u32, flags, None, Some(buf.as_mut_ptr() as _), &mut size) != 0 {
            buf = vec![0u8; size as usize];
            if GetAdaptersAddresses(AF_INET.0 as u32, flags, None, Some(buf.as_mut_ptr() as _), &mut size) != 0 {
                return Link::None;
            }
        }
        let mut kind = Link::None;
        let mut a = buf.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
        while !a.is_null() {
            let ad = &*a;
            a = ad.Next;
            if ad.OperStatus != IfOperStatusUp || ad.IfType == 24 {
                continue; // down, or the loopback
            }
            let mut u = ad.FirstUnicastAddress;
            let mut real = false;
            while !u.is_null() {
                let sa = (*u).Address.lpSockaddr;
                if !sa.is_null() && (*sa).sa_family == AF_INET {
                    let ip = (*(sa as *const SOCKADDR_IN)).sin_addr.S_un.S_un_b;
                    real |= !(ip.s_b1 == 169 && ip.s_b2 == 254) && ip.s_b1 != 127;
                }
                u = (*u).Next;
            }
            if !real {
                continue;
            }
            if ad.IfType == 71 {
                return Link::Wifi; // IEEE 802.11
            }
            kind = Link::Ethernet;
        }
        kind
    }
}

/// Windows' own connectivity check: does the internet answer?
fn probe() -> bool {
    ureq::get("http://www.msftconnecttest.com/connecttest.txt")
        .timeout(Duration::from_secs(4))
        .call()
        .ok()
        .and_then(|r| r.into_string().ok())
        .is_some_and(|t| t.starts_with("Microsoft Connect Test"))
}

/// On the charger, and the charge in percent; None without a battery.
fn battery_now() -> Option<(bool, u8)> {
    use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    let mut s = SYSTEM_POWER_STATUS::default();
    unsafe { GetSystemPowerStatus(&mut s).ok()? };
    if s.BatteryFlag & 128 != 0 || s.BatteryLifePercent > 100 {
        return None;
    }
    Some((s.ACLineStatus == 1, s.BatteryLifePercent))
}

fn endpoint_volume() -> Option<windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume> {
    use windows::Win32::Media::Audio::{eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator};
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};
    unsafe {
        let e: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
        let dev = e.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
        dev.Activate(CLSCTX_ALL, None).ok()
    }
}

thread_local! {
    /// The speakers' volume control, kept for the slider and the wheel (looked up again
    /// when the default device changes).
    static SPEAKERS: std::cell::RefCell<Option<windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume>> = const { std::cell::RefCell::new(None) };
}

/// Runs `f` on the speakers' volume control (looking it up again once if it fails).
fn with_speakers<T>(f: impl Fn(&windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume) -> windows::core::Result<T>) -> Option<T> {
    SPEAKERS.with(|s| {
        let mut s = s.borrow_mut();
        if s.is_none() {
            *s = endpoint_volume();
        }
        if let Some(r) = s.as_ref().and_then(|v| f(v).ok()) {
            return Some(r);
        }
        *s = endpoint_volume();
        s.as_ref().and_then(|v| f(v).ok())
    })
}

/// Sets the speakers' volume (0 to 1); above nothing it also unmutes them.
pub fn set_volume(level: f32) -> Option<f32> {
    let level = level.clamp(0.0, 1.0);
    with_speakers(|v| unsafe {
        v.SetMasterVolumeLevelScalar(level, std::ptr::null())?;
        if level > 0.0 {
            v.SetMute(false, std::ptr::null())?;
        }
        Ok(level)
    })
}

/// Mutes the speakers, or turns them back on. The new state, if it worked.
pub fn toggle_speakers() -> Option<bool> {
    with_speakers(|v| unsafe {
        let muted = !v.GetMute()?.as_bool();
        v.SetMute(muted, std::ptr::null())?;
        Ok(muted)
    })
}

/// The default microphone's volume control (its mute is the whole PC's mute for it).
pub fn mic_volume() -> Option<windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume> {
    use windows::Win32::Media::Audio::{eCapture, eConsole, IMMDeviceEnumerator, MMDeviceEnumerator};
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};
    unsafe {
        let e: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
        let dev = e.GetDefaultAudioEndpoint(eCapture, eConsole).ok()?;
        dev.Activate(CLSCTX_ALL, None).ok()
    }
}

/// Mutes the default microphone, or turns it back on. The new state, if it worked.
pub fn toggle_mic() -> Option<bool> {
    let m = mic_volume()?;
    unsafe {
        let muted = m.GetMute().ok()?.as_bool();
        m.SetMute(!muted, std::ptr::null()).ok()?;
        Some(!muted)
    }
}

fn wmi_connect() -> Option<windows::Win32::System::Wmi::IWbemServices> {
    use windows::core::BSTR;
    use windows::Win32::System::Com::*;
    use windows::Win32::System::Wmi::*;
    unsafe {
        let loc: IWbemLocator = CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER).ok()?;
        let svc = loc.ConnectServer(&BSTR::from("root\\WMI"), &BSTR::new(), &BSTR::new(), &BSTR::new(), 0, &BSTR::new(), None).ok()?;
        let _ = CoSetProxyBlanket(&svc, 10, 0, None, RPC_C_AUTHN_LEVEL_CALL, RPC_C_IMP_LEVEL_IMPERSONATE, None, EOAC_NONE);
        Some(svc)
    }
}

/// The panel's brightness (laptops; external screens have none in software).
fn brightness(svc: &windows::Win32::System::Wmi::IWbemServices) -> Option<u32> {
    use windows::core::{BSTR, w};
    use windows::core::VARIANT;
    use windows::Win32::System::Wmi::*;
    unsafe {
        let q = svc
            .ExecQuery(&BSTR::from("WQL"), &BSTR::from("SELECT CurrentBrightness FROM WmiMonitorBrightness"), WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY, None)
            .ok()?;
        let mut objs = [None];
        let mut got = 0;
        let _ = q.Next(2000, &mut objs, &mut got);
        let obj = objs[0].take()?;
        let mut v = VARIANT::default();
        obj.Get(w!("CurrentBrightness"), 0, &mut v, None, None).ok()?;
        u32::try_from(&v).ok().or_else(|| i32::try_from(&v).ok().map(|i| i.max(0) as u32))
    }
}

/// The keyboard layout of the window in front (a Windows language id), and that window's
/// process, so a switch of windows (with its own layout) isn't taken for a change.
pub fn keyboard_layout() -> (u32, u32) {
    use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyboardLayout;
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    unsafe {
        let fg = GetForegroundWindow();
        let mut pid = 0u32;
        let thread = GetWindowThreadProcessId(fg, Some(&mut pid));
        ((GetKeyboardLayout(thread).0 as usize & 0xffff) as u32, pid)
    }
}

pub fn caps_lock() -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CAPITAL};
    unsafe { GetKeyState(VK_CAPITAL.0 as i32) & 1 != 0 }
}

/// A language id's name and badge (by its primary language), as Nova shows them.
pub fn language(kl: u32) -> (String, String) {
    let t: (&str, &str) = match kl & 0x3ff {
        0x01 => ("\u{0627}\u{0644}\u{0639}\u{0631}\u{0628}\u{064a}\u{0629}", "\u{0639}"),
        0x09 => ("English", "EN"),
        0x0c => ("Fran\u{00e7}ais", "FR"),
        0x07 => ("Deutsch", "DE"),
        0x0a => ("Espa\u{00f1}ol", "ES"),
        0x10 => ("Italiano", "IT"),
        0x1f => ("T\u{00fc}rk\u{00e7}e", "TR"),
        0x29 => ("\u{0641}\u{0627}\u{0631}\u{0633}\u{06cc}", "\u{0641}\u{0627}"),
        0x20 => ("\u{0627}\u{0631}\u{062f}\u{0648}", "\u{0627}\u{0631}"),
        0x19 => ("\u{0420}\u{0443}\u{0441}\u{0441}\u{043a}\u{0438}\u{0439}", "RU"),
        0x16 => ("Portugu\u{00ea}s", "PT"),
        0x0d => ("\u{05e2}\u{05d1}\u{05e8}\u{05d9}\u{05ea}", "HE"),
        _ => return (format!("0x{kl:x}"), "\u{2328}".to_owned()),
    };
    (t.0.to_owned(), t.1.to_owned())
}
