//! Embeds Lulo's icon in the Windows .exe.

fn main() {
    println!("cargo:rerun-if-changed=../../assets/lulo.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../assets/lulo.ico");
        res.compile().expect("could not embed the icon");
    }
}
