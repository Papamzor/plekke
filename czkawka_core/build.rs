fn main() {
    let rust_version = match rustc_version::version_meta() {
        Ok(meta) => format!("{} ({})", meta.semver, meta.commit_date.unwrap_or_default()),
        Err(_) => "<unknown>".to_string(),
    };
    println!("cargo:rustc-env=RUST_VERSION_INTERNAL={rust_version}");
    println!("cargo:rustc-env=CZKAWKA_GIT_COMMIT_SHORT={}", git_output(&["rev-parse", "--short=10", "HEAD"]));
    println!(
        "cargo:rustc-env=CZKAWKA_GIT_COMMIT_DATE={}",
        git_output(&["log", "-1", "--format=%cd", "--date=format:%d/%m/%Y"])
    );

    let using_cranelift =
        std::env::var("CARGO_PROFILE_DEV_CODEGEN_BACKEND").as_deref() == Ok("cranelift") || std::env::var("CARGO_PROFILE_RELEASE_CODEGEN_BACKEND").as_deref() == Ok("cranelift");
    if using_cranelift {
        println!("cargo:rustc-env=USING_CRANELIFT=1");
    }
}

fn git_output(args: &[&str]) -> String {
    std::process::Command::new("git")
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map_or_else(|| "<unknown>".to_string(), |s| s.trim().to_string())
}
