fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let out = rugsnipe_ctl::run(&args);
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
