fn main() {
    let exit_code = twoexcamim::runtime::run(
        std::env::args(),
        &mut std::io::stdout(),
        &mut std::io::stderr(),
    );
    std::process::exit(exit_code);
}
