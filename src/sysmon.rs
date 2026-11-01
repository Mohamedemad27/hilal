//! Live system stats for the System page: CPU, RAM, GPU (through NVIDIA's NVML when
//! present) and network throughput. Sampled on a background thread, only while the page
//! is visible, so the notch's frames never wait on it.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows::core::{s, w};
use windows::Win32::Foundation::{FreeLibrary, FILETIME, HMODULE};
use windows::Win32::NetworkManagement::IpHelper::{FreeMibTable, GetIfTable2, MIB_IF_TABLE2};
use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::System::SystemInformation::{GetSystemInfo, GlobalMemoryStatusEx, MEMORYSTATUSEX, SYSTEM_INFO};
use windows::Win32::System::Threading::GetSystemTimes;

const PERIOD: Duration = Duration::from_millis(500);

#[derive(Clone, Default)]
pub struct Snapshot {
    /// 0..1
    pub cpu: f32,
    pub threads: u32,
    pub ram_used: u64,
    pub ram_total: u64,
    /// 0..1, None without a supported GPU.
    pub gpu: Option<f32>,
    pub gpu_temp: Option<u32>,
    pub gpu_name: String,
    pub vram: Option<(u64, u64)>,
    /// Bytes per second.
    pub net_down: f64,
    pub net_up: f64,
    /// False until two samples exist (rates need a delta).
    pub ready: bool,
}

pub struct Sysmon {
    snap: Arc<Mutex<Snapshot>>,
    active: Arc<AtomicBool>,
}

impl Sysmon {
    pub fn start() -> Self {
        let snap = Arc::new(Mutex::new(Snapshot::default()));
        let active = Arc::new(AtomicBool::new(false));
        let (s, a) = (snap.clone(), active.clone());
        std::thread::spawn(move || run(s, a));
        Self { snap, active }
    }

    pub fn set_active(&self, on: bool) {
        self.active.store(on, Ordering::Relaxed);
    }

    pub fn get(&self) -> Snapshot {
        self.snap.lock().unwrap().clone()
    }
}

fn run(snap: Arc<Mutex<Snapshot>>, active: Arc<AtomicBool>) {
    let nvml = Nvml::load();
    let threads = unsafe {
        let mut info = SYSTEM_INFO::default();
        GetSystemInfo(&mut info);
        info.dwNumberOfProcessors
    };
    let mut prev: Option<(Instant, CpuTimes, (u64, u64))> = None;
    loop {
        if !active.load(Ordering::Relaxed) {
            prev = None;
            std::thread::sleep(Duration::from_millis(250));
            continue;
        }
        let now = Instant::now();
        let cpu_t = cpu_times();
        let net = net_octets();
        let mut s = Snapshot { threads, ..Default::default() };
        if let (Some((t0, c0, n0)), Some(c1)) = (&prev, cpu_t) {
            let total = (c1.kernel + c1.user).saturating_sub(c0.kernel + c0.user);
            let idle = c1.idle.saturating_sub(c0.idle);
            s.cpu = if total > 0 { 1.0 - idle as f32 / total as f32 } else { 0.0 };
            let secs = now.duration_since(*t0).as_secs_f64().max(0.001);
            s.net_down = net.0.saturating_sub(n0.0) as f64 / secs;
            s.net_up = net.1.saturating_sub(n0.1) as f64 / secs;
            s.ready = true;
        }
        unsafe {
            let mut mem = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
            if GlobalMemoryStatusEx(&mut mem).is_ok() {
                s.ram_total = mem.ullTotalPhys;
                s.ram_used = mem.ullTotalPhys - mem.ullAvailPhys;
            }
        }
        if let Some(n) = &nvml {
            n.sample(&mut s);
        }
        if let Some(c) = cpu_t {
            prev = Some((now, c, net));
        }
        *snap.lock().unwrap() = s;
        std::thread::sleep(PERIOD);
    }
}

#[derive(Clone, Copy)]
struct CpuTimes {
    idle: u64,
    kernel: u64,
    user: u64,
}

fn cpu_times() -> Option<CpuTimes> {
    let (mut i, mut k, mut u) = (FILETIME::default(), FILETIME::default(), FILETIME::default());
    unsafe { GetSystemTimes(Some(&mut i), Some(&mut k), Some(&mut u)).ok()? };
    let v = |f: FILETIME| ((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64;
    // Kernel time includes idle time.
    Some(CpuTimes { idle: v(i), kernel: v(k), user: v(u) })
}

/// Total bytes in/out across physical network adapters that are up.
fn net_octets() -> (u64, u64) {
    unsafe {
        let mut table: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
        if GetIfTable2(&mut table).is_err() || table.is_null() {
            return (0, 0);
        }
        let n = (*table).NumEntries as usize;
        let rows = std::slice::from_raw_parts((*table).Table.as_ptr(), n);
        let (mut down, mut up) = (0u64, 0u64);
        for r in rows {
            let hardware = r.InterfaceAndOperStatusFlags._bitfield & 1 != 0;
            if hardware && r.OperStatus == IfOperStatusUp {
                down += r.InOctets;
                up += r.OutOctets;
            }
        }
        FreeMibTable(table as *const c_void);
        (down, up)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_cpu_and_network() {
        let a = cpu_times().expect("GetSystemTimes");
        std::thread::sleep(Duration::from_millis(200));
        let b = cpu_times().expect("GetSystemTimes");
        assert!(b.kernel + b.user > a.kernel + a.user);
        let (down, up) = net_octets();
        println!("cpu ok; net totals down={down} up={up}");
    }
}

// ----- NVML (NVIDIA Management Library), loaded at runtime ------------------------------

#[repr(C)]
#[derive(Default)]
struct NvUtil {
    gpu: u32,
    memory: u32,
}

#[repr(C)]
#[derive(Default)]
struct NvMem {
    total: u64,
    free: u64,
    used: u64,
}

type Dev = *mut c_void;

struct Nvml {
    _lib: HMODULE,
    dev: Dev,
    name: String,
    util: unsafe extern "C" fn(Dev, *mut NvUtil) -> i32,
    temp: unsafe extern "C" fn(Dev, u32, *mut u32) -> i32,
    mem: unsafe extern "C" fn(Dev, *mut NvMem) -> i32,
}

// The handle is only used from the sampler thread that created it.
unsafe impl Send for Nvml {}

impl Nvml {
    fn load() -> Option<Self> {
        unsafe {
            let lib = LoadLibraryW(w!("nvml.dll")).ok()?;
            let get = |name| GetProcAddress(lib, name);
            let loaded = (|| {
                let init: unsafe extern "C" fn() -> i32 = std::mem::transmute(get(s!("nvmlInit_v2"))?);
                let by_index: unsafe extern "C" fn(u32, *mut Dev) -> i32 = std::mem::transmute(get(s!("nvmlDeviceGetHandleByIndex_v2"))?);
                let name_fn: unsafe extern "C" fn(Dev, *mut u8, u32) -> i32 = std::mem::transmute(get(s!("nvmlDeviceGetName"))?);
                let util = std::mem::transmute(get(s!("nvmlDeviceGetUtilizationRates"))?);
                let temp = std::mem::transmute(get(s!("nvmlDeviceGetTemperature"))?);
                let mem = std::mem::transmute(get(s!("nvmlDeviceGetMemoryInfo"))?);
                if init() != 0 {
                    return None;
                }
                let mut dev: Dev = std::ptr::null_mut();
                if by_index(0, &mut dev) != 0 {
                    return None;
                }
                let mut buf = [0u8; 96];
                let name = if name_fn(dev, buf.as_mut_ptr(), buf.len() as u32) == 0 {
                    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
                    String::from_utf8_lossy(&buf[..end]).into_owned()
                } else {
                    String::new()
                };
                Some(Nvml { _lib: lib, dev, name, util, temp, mem })
            })();
            if loaded.is_none() {
                let _ = FreeLibrary(lib);
            }
            loaded
        }
    }

    fn sample(&self, s: &mut Snapshot) {
        unsafe {
            let mut u = NvUtil::default();
            if (self.util)(self.dev, &mut u) == 0 {
                s.gpu = Some(u.gpu as f32 / 100.0);
            }
            let mut t = 0u32;
            if (self.temp)(self.dev, 0, &mut t) == 0 {
                s.gpu_temp = Some(t);
            }
            let mut m = NvMem::default();
            if (self.mem)(self.dev, &mut m) == 0 {
                s.vram = Some((m.used, m.total));
            }
        }
        s.gpu_name = self.name.clone();
    }
}
