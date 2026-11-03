//! Donanım okumalarını konsola döker: `cargo run -p carlui-sys --example probe`.

fn main() {
    let mut cpu = carlui_sys::cpu::Cpu::new();
    let mut dm = carlui_sys::disk::Meter::new();
    let mut disks = carlui_sys::disk::list();
    std::thread::sleep(std::time::Duration::from_millis(1000));
    let s = cpu.sample();
    dm.update(&mut disks);
    println!("CPU {} ({} çekirdek): %{:.1} {:.0} MHz", cpu.name, cpu.physical, s.usage, s.mhz);
    for c in &s.cores {
        println!("  lp{:02} çekirdek {:02} {:?} %{:.0} {:.0} MHz", c.index, c.core, c.kind, c.usage, c.mhz);
    }
    let r = carlui_sys::ram::read();
    println!("RAM {} / {} (%{:.0}) commit {} / {}", carlui_sys::bytes(r.used), carlui_sys::bytes(r.total), r.percent(), carlui_sys::bytes(r.commit), carlui_sys::bytes(r.commit_limit));
    println!("GPU {:?}", carlui_sys::gpu::read());
    println!("CPU sıcaklık {:?}", carlui_sys::temp::read());
    for d in &disks {
        println!("Disk {} {} {} ssd={} {:?} {} / {} sıcaklık {:?} sağlık {:?} etkin %{:.0} R {} W {}", d.index, d.model, d.bus, d.ssd, d.volumes, carlui_sys::bytes(d.total - d.free), carlui_sys::bytes(d.total), d.temp, d.health, d.active, carlui_sys::bytes(d.read_bps), carlui_sys::bytes(d.write_bps));
    }
}
