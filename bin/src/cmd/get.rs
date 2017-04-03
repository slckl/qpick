use std::io;

use docopt::Docopt;

use qi;
use fst::Map;

use std::io::Read;
use std::fs::File;
use std::result::Result;

use serde_json;
use std::fs::OpenOptions;

use Error;

const USAGE: &'static str = "
Get vector ids and scores for ANN.
Usage:
    mann get [options] <query>
    mann get --help
Options:
    -h, --help  Arg qurey is a query string.
";

#[derive(Debug, RustcDecodable)]
struct Args {
    arg_query: String,
}

pub fn run(argv: Vec<String>) -> Result<(), Error> {
    let args: Args = Docopt::new(USAGE)
                            .and_then(|d| d.argv(&argv).decode())
                            .unwrap_or_else(|e| e.exit());

    let qi = qi::Qi::from_path("./index".to_string());
    let r = qi.search(&args.arg_query);
    println!("{:?}", r);

    Ok(())
}