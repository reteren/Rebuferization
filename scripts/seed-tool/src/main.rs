use std::io::Read;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: seed-tool <text|raw> <file>");
        std::process::exit(2);
    }
    let mode = &args[1];
    let path = &args[2];
    let mut data = Vec::new();
    std::fs::File::open(path)
        .expect("open file")
        .read_to_end(&mut data)
        .expect("read file");
    let hash = if mode == "text" {
        let normalized = String::from_utf8_lossy(&data).replace("\r\n", "\n").replace('\r', "\n");
        let normalized = normalized.trim_end().to_string();
        blake3::hash(normalized.as_bytes()).to_hex().to_string()
    } else {
        blake3::hash(&data).to_hex().to_string()
    };
    println!("{hash}");
}