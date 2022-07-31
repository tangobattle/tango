use mmpx;
use structopt::StructOpt;

#[derive(StructOpt)]
struct Cli {
    // The path to the file to read
    #[structopt(parse(from_os_str))]
    path: std::path::PathBuf,
    #[structopt(short = "o", long = "output", default_value = "output.png")]
    output: std::path::PathBuf,
}

fn main() {
    let args = Cli::from_args();

    let input_image = image::open(args.path).unwrap().to_rgba8();

    let output_image = mmpx::magnify(&input_image);

    output_image.save(args.output).unwrap();
}