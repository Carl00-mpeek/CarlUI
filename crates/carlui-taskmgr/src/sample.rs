//! Saniyede bir ölçüm ve son 60 saniyenin geçmişi (grafikler için). Disk
//! listesi (model, sıcaklık, sağlık) cihazları açtığı için 10 saniyede bir
//! yenilenir; etkinlik ve hızlar her saniye.

use carlui_sys::{cpu, disk, gpu, ram, temp};
use std::collections::VecDeque;

pub const LEN: usize = 60;

#[derive(Clone, Default)]
pub struct History(pub VecDeque<f32>);

impl History {
    pub fn push(&mut self, v: f32) {
        if self.0.len() >= LEN {
            self.0.pop_front();
        }
        self.0.push_back(v);
    }
}

pub struct Sampler {
    cpu: cpu::Cpu,
    meter: disk::Meter,
    ticks: u32,
    pub name: String,
    pub physical: u32,
    pub now: cpu::Sample,
    pub cpu_hist: History,
    pub core_hist: Vec<History>,
    pub temps: Option<temp::Temps>,
    pub gpu: Option<gpu::Gpu>,
    pub gpu_hist: History,
    pub vram_hist: History,
    pub ram: ram::Ram,
    pub ram_hist: History,
    pub disks: Vec<disk::Disk>,
    pub disk_hist: Vec<History>,
}

impl Sampler {
    pub fn new() -> Self {
        let cpu = cpu::Cpu::new();
        let mut s = Self {
            name: cpu.name.clone(),
            physical: cpu.physical,
            cpu,
            meter: disk::Meter::new(),
            ticks: 0,
            now: Default::default(),
            cpu_hist: History::default(),
            core_hist: Vec::new(),
            temps: None,
            gpu: None,
            gpu_hist: History::default(),
            vram_hist: History::default(),
            ram: Default::default(),
            ram_hist: History::default(),
            disks: disk::list(),
            disk_hist: Vec::new(),
        };
        s.tick();
        s
    }

    pub fn tick(&mut self) {
        self.now = self.cpu.sample();
        self.cpu_hist.push(self.now.usage);
        self.core_hist.resize(self.now.cores.len(), History::default());
        for (h, c) in self.core_hist.iter_mut().zip(&self.now.cores) {
            h.push(c.usage);
        }
        self.temps = temp::read();
        self.gpu = gpu::read();
        if let Some(g) = &self.gpu {
            self.gpu_hist.push(g.load);
            self.vram_hist.push(if g.vram_total > 0.0 { (g.vram_used / g.vram_total * 100.0) as f32 } else { 0.0 });
        }
        self.ram = ram::read();
        self.ram_hist.push(self.ram.percent());
        self.ticks += 1;
        if self.ticks % 10 == 0 {
            self.disks = disk::list();
        }
        self.meter.update(&mut self.disks);
        self.disk_hist.resize(self.disks.len(), History::default());
        for (h, d) in self.disk_hist.iter_mut().zip(&self.disks) {
            h.push(d.active);
        }
    }

    /// Fiziksel çekirdeğin sıcaklığı (sürücü kuruluysa).
    /// Okuyucu sıcaklıkları fiziksel çekirdek numarası sırasıyla yazar.
    pub fn core_temp(&self, core: u32) -> Option<f32> {
        let mut ids: Vec<u32> = self.now.cores.iter().map(|c| c.core).collect();
        ids.sort();
        ids.dedup();
        let pos = ids.iter().position(|&c| c == core)?;
        self.temps.as_ref().and_then(|t| t.cores.get(pos).copied()).filter(|t| *t > 0.0)
    }
}
