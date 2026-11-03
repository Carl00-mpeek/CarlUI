//! İşlem tablosu: çekirdekten gelen süreç listesinden (carlui_sys::process)
//! iki ölçüm arasındaki farkla CPU yüzdesi ve G/Ç hızı hesaplanır. Exe yolu
//! (pid, oluşturulma zamanı) başına bir kez okunur; aynı pid'i yeni bir süreç
//! alırsa yeniden okunur.

use crate::util::is_windows_path;
use carlui_sys::process;
use std::{collections::HashMap, time::Instant};

#[derive(Clone, Debug, Default)]
pub struct Row {
    pub pid: u32,
    pub name: String,
    pub path: String,
    pub cpu: f32,
    pub mem: u64,
    pub io: f64,
    pub gpu: f32,
    pub threads: u32,
    pub suspended: bool,
    pub windows: bool,
}

pub struct Procs {
    prev: HashMap<u32, (i64, u64)>,
    paths: HashMap<(u32, i64), String>,
    last: Instant,
    ncpu: f32,
    gpu: carlui_sys::gpu_procs::Meter,
    pub rows: Vec<Row>,
}

impl Procs {
    pub fn new() -> Self {
        let mut p = Self { prev: HashMap::new(), paths: HashMap::new(), last: Instant::now(), ncpu: carlui_sys::cpu::logical_count().max(1) as f32, gpu: carlui_sys::gpu_procs::Meter::new(), rows: Vec::new() };
        p.tick();
        p
    }

    pub fn tick(&mut self) {
        let list = process::list();
        let gpu = self.gpu.read();
        let dt = self.last.elapsed().as_secs_f64().max(0.05);
        self.last = Instant::now();
        let mut prev = HashMap::with_capacity(list.len());
        let mut rows = Vec::with_capacity(list.len());
        let mut live = HashMap::with_capacity(list.len());
        for p in list {
            let io = p.read_bytes + p.write_bytes;
            let (cpu, iops) = match self.prev.get(&p.pid) {
                Some(&(t, b)) if p.pid != 0 => (((p.cpu_time - t).max(0) as f64 / (dt * 1e7) / self.ncpu as f64 * 100.0) as f32, io.saturating_sub(b) as f64 / dt),
                _ => (0.0, 0.0),
            };
            prev.insert(p.pid, (p.cpu_time, io));
            let key = (p.pid, p.create_time);
            let path = match self.paths.get(&key) {
                Some(s) => s.clone(),
                None => {
                    let s = if p.pid <= 4 { String::new() } else { process::exe_path(p.pid) };
                    s
                }
            };
            live.insert(key, path.clone());
            if p.pid == 0 {
                continue;
            }
            rows.push(Row { windows: path.is_empty() || is_windows_path(&path), pid: p.pid, name: p.name, path, cpu: cpu.clamp(0.0, 100.0), mem: p.working_set, io: iops, gpu: gpu.get(&p.pid).copied().unwrap_or(0.0), threads: p.threads, suspended: p.suspended });
        }
        self.paths = live;
        self.prev = prev;
        self.rows = rows;
    }
}
