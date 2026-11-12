fn main() {
    let config = slint_build::CompilerConfiguration::new()
        .with_style("fluent".into())
        .embed_resources(slint_build::EmbedResourcesKind::EmbedForSoftwareRenderer);
    slint_build::compile_with_config("ui/app.slint", config).unwrap();
    println!("cargo:rerun-if-changed=ui/app.slint");
    println!("cargo:rerun-if-changed=ui/common.slint");
    println!("cargo:rerun-if-changed=ui/panels.slint");
    println!("cargo:rerun-if-changed=ui/popups.slint");
}
