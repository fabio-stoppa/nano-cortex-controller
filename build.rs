use std::env;
use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let gen_path = manifest_dir.join("sdkconfig.gen.defaults");
    let partitions_csv = manifest_dir.join("partitions.csv");
    let body = format!(
        "CONFIG_PARTITION_TABLE_CUSTOM=y\nCONFIG_PARTITION_TABLE_CUSTOM_FILENAME=\"{}\"\n",
        partitions_csv.display()
    );
    if std::fs::read_to_string(&gen_path).ok().as_deref() != Some(body.as_str()) {
        std::fs::write(&gen_path, &body).expect("write sdkconfig.gen.defaults");
    }

    embuild::espidf::sysenv::output();
}
