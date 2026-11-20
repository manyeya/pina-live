//! Windows: Pina Live's icon and its details (product, company, copyright, description) built
//! into the program, as Properties › Details shows them. Nothing to do elsewhere.

fn main() {
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("icon/live.ico")
            .set("ProductName", "Pina Live")
            .set(
                "FileDescription",
                "Pina Live: DJ software on Pina Studio's engine",
            )
            .set("CompanyName", "Manyeya Khutso Siema")
            .set(
                "LegalCopyright",
                "Copyright (c) 2026 Manyeya Khutso Siema. MIT License.",
            )
            .set("OriginalFilename", "Pina Live.exe");
        if let Err(e) = res.compile() {
            println!("cargo:warning=no Windows resources: {e}");
        }
    }
}
