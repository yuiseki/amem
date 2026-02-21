fn main() {
    if let Err(err) = amem::run_cli() {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}
