fn main() {
    #[cfg(windows)]
    {
        use std::io::Write;
        let mut res = winres::WindowsResource::new();
        if cfg!(feature = "ongrow-support-desk") {
            res.set_icon("../../flutter/windows/runner/resources/app_icon.ico")
                .set("CompanyName", "OnGROW GmbH")
                .set("ProductName", "OnGROW Support Desk")
                .set("InternalName", "ongrow_support_desk_setup")
                .set("OriginalFilename", "OnGROW Support Desk Setup.exe")
                .set("FileDescription", "OnGROW Support Desk Setup")
                .set("LegalCopyright", "Copyright OnGROW GmbH");
        } else {
            res.set_icon("../../res/icon.ico");
        }
        res.set_language(winapi::um::winnt::MAKELANGID(
                winapi::um::winnt::LANG_ENGLISH,
                winapi::um::winnt::SUBLANG_ENGLISH_US,
            ))
            .set_manifest_file("../../res/manifest.xml");
        match res.compile() {
            Err(e) => {
                let _ = write!(std::io::stderr(), "{}", e);
                std::process::exit(1);
            }
            Ok(_) => {}
        }
    }
}
