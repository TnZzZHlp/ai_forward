fn main() {
    // 使用编译时的时间戳
    // 注意：这将在每次编译时更新
    let now = chrono::Local::now();
    let build_time = now.format("%Y-%m-%d %H:%M:%S").to_string();

    println!("cargo:rustc-env=BUILD_TIME={}", build_time);

    // 让 Cargo 在每次构建时都重新运行这个脚本
    println!("cargo:rerun-if-changed=build.rs");
}
