use docopt::Docopt;

use qpick;

use std::result::Result;

use Error;

const USAGE: &'static str = "
Get vector ids and scores for ANN.

Usage:
    qpick get [options] <query> <count> [--with-tfidf]
    qpick get --help

Options:
    -h, --help         Arg query is a query string.
    -s, --start ARG    Shard to begin with.
    -e, --end ARG      Shard to end with goes together with the --start option.
";

#[derive(Debug, Deserialize)]
struct Args {
    flag_start: Option<u32>,
    flag_end: Option<u32>,
    arg_query: String,
    arg_count: u32,
    flag_with_tfidf: bool,
}

pub fn run(argv: Vec<String>) -> Result<(), Error> {
    let args: Args = Docopt::new(USAGE)
        .and_then(|d| d.argv(&argv).deserialize())
        .unwrap_or_else(|e| e.exit());

    let qpick: qpick::Qpick;

    println!("{:?}", args);
    if let Some(start_shard) = args.flag_start {
        if let Some(end_shard) = args.flag_end {
            assert!(end_shard > start_shard);
            qpick = qpick::Qpick::from_path_with_shard_range(
                "./index".to_string(),
                start_shard..end_shard,
            );
        } else {
            panic!("Missing the end shard value! Run --help for more info!")
        }
    } else {
        qpick = qpick::Qpick::from_path("./index".to_string());
    }

    let r = qpick.get(&args.arg_query, args.arg_count, args.flag_with_tfidf);

    let v: Vec<(u64, Option<f32>, f32, String)> = r
        .iter()
        .map(|r| {
            let q = if let Some(ref query) = r.query {
                query
            } else {
                ""
            };

            (r.query_id, r.dist.cosine, r.dist.keyword, q.to_string())
        })
        .collect();

    println!("{:?}", v);

    Ok(())
}
