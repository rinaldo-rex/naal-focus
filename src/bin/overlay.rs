fn main() {
    let result = naal_focus::overlay::Options::parse(std::env::args().skip(1))
        .map_err(|e| e.into())
        .and_then(|options| match options {
            Some(options) => naal_focus::overlay::run(options),
            None => {
                print!("{}", naal_focus::overlay::HELP);
                Ok(())
            }
        });
    if let Err(error) = result {
        eprintln!("Naal Focus: {error}");
        std::process::exit(1);
    }
}
