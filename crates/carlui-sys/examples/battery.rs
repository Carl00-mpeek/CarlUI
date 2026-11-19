fn main() {
    let t = std::time::Instant::now();
    println!("{:#?}", carlui_sys::battery::read_all());
    println!("{:?}", t.elapsed());
}
