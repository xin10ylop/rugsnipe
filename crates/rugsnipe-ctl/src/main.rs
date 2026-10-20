fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // progress rows (`nonce create`) go to stdout as they happen
    let out = rugsnipe_ctl::run_streaming(&args, &mut std::io::stdout());
    if out.code == 0 {
        print!("{}", out.text);
    } else {
        eprint!("{}", out.text);
        if !out.text.ends_with('\n') {
            eprintln!();
        }
    }
    std::process::exit(out.code);
}
