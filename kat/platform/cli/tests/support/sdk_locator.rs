fn main() {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    if arguments.get(4).map(String::as_str) == Some("-c") {
        let location = std::env::current_exe().unwrap().parent().unwrap().join("sdk-fixture");
        print!("{:?}", location.to_str().unwrap());
        return;
    }
    std::process::exit(91);
}
