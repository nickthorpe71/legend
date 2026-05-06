fn main() {
    if let Err(e) = legend::run() {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}
