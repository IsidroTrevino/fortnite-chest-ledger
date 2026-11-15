use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=ledger.rc");
    println!("cargo:rerun-if-changed=../assets/treasure-chest.ico");
    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("ledger.res");
    let status = Command::new("rc.exe").arg("/nologo").arg("/fo").arg(&output)
        .arg("ledger.rc").status().expect("Run Cargo from a Visual Studio Developer PowerShell with the Windows SDK");
    assert!(status.success(), "The Windows icon resource did not compile");
    println!("cargo:rustc-link-arg={}", output.display());
}
