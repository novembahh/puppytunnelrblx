fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/icon/puppytunnel.ico");
        res.set("ProductName", "PuppyTunnel");
        res.set("FileDescription", "PuppyTunnel");
        res.compile().expect("failed to embed the exe icon");
    }
    println!("cargo:rerun-if-changed=assets/icon/puppytunnel.ico");
}
