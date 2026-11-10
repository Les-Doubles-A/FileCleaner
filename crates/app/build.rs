fn main() {
    println!("cargo:rerun-if-changed=../../assets/FileCleaner.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("../../assets/FileCleaner.ico");
        resource.set("FileDescription", "File Cleaner");
        resource.set("ProductName", "File Cleaner");
        if let Err(err) = resource.compile() {
            println!("cargo:warning=icone non integree : {err}");
        }
    }
}
