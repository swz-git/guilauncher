extern crate winres;

fn main() {
    static_vcruntime::metabuild();
    if cfg!(target_os = "windows") {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/logo.ico");
        res.compile().unwrap();
    }
}
