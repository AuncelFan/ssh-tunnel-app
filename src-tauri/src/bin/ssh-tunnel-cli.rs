use ssh_tunnel::cli::{self, Entry};

fn main() {
    std::process::exit(cli::execute(&cli::arguments(), Entry::Console));
}
