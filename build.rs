//! On Windows, put the app's icon and its details inside the program file:
//! that is where Explorer, the taskbar and the window itself look for them.
//! Nothing happens here on other systems.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/ZinMD.ico");
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        windows::embed();
    }
}

#[cfg(windows)]
mod windows {
    use std::path::PathBuf;

    pub fn embed() {
        let root = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
        let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
        let version = std::env::var("CARGO_PKG_VERSION").unwrap();
        // The system wants four plain numbers: 0.1.0-rc.1 is 0,1,0,0.
        let mut numbers: Vec<&str> = version
            .split('-')
            .next()
            .unwrap_or_default()
            .split('.')
            .collect();
        numbers.resize(4, "0");
        let numbers = numbers.join(",");
        // The resource compiler reads backslashes as the start of an escape.
        let icon = root
            .join("assets/ZinMD.ico")
            .to_string_lossy()
            .replace('\\', "/");

        // The window takes its icon from the one numbered 1.
        let script = format!(
            r#"1 ICON "{icon}"
1 VERSIONINFO
FILEVERSION {numbers}
PRODUCTVERSION {numbers}
BEGIN
    BLOCK "StringFileInfo"
    BEGIN
        BLOCK "040904B0"
        BEGIN
            VALUE "ProductName", "ZinMD"
            VALUE "FileDescription", "ZinMD"
            VALUE "CompanyName", "ZinMD"
            VALUE "LegalCopyright", "MIT licence"
            VALUE "OriginalFilename", "ZinMD.exe"
            VALUE "FileVersion", "{version}"
            VALUE "ProductVersion", "{version}"
        END
    END
    BLOCK "VarFileInfo"
    BEGIN
        VALUE "Translation", 0x409, 1200
    END
END
"#
        );
        let file = out.join("zinmd.rc");
        std::fs::write(&file, script).expect("could not write the resource script");
        embed_resource::compile(&file, embed_resource::NONE)
            .manifest_optional()
            .expect("could not compile the app's icon into the program");
    }
}
