use std::io::Error;
use std::fs::File;
use std::io::{BufReader, BufWriter, Write};
use std::io::prelude::*;
use fst::Map;

use std::fs::OpenOptions;

use util;
use config;
use stopwords;
use ngrams;

use std::fs;
use std::thread;
use std::sync::mpsc;
use std::sync::mpsc::{Receiver, Sender};

use std::collections::HashMap;

const WRITE_BUFFER_SIZE: usize = 5 * 1024;

/*

Reads data from a single input file by multiple threads so that each i-th thread
reads every i-th row in the file. This should have been done with multi threaded
one-sender-many-receivers message passing, w/o broadcasting, but this hasn't yet
been supported in the rust's std library.

TODO is there a way to use Rayon similar to this( https://github.com/rayon-rs/rayon/issues/297 ):

    let input_file = File::open(&input_path).unwrap();
    let mut reader = io::BufReader::new(input_file);
    let result = reader.lines().par_iter();

- batching lines to vector and then par_iter them doesn't look better than the
current solution ( https://github.com/rayon-rs/rayon/issues/46 ):

    let buffer = String::new();
    File::open(some_path)?.read_to_string(buffer)?;          // iterate unindexed data once
    let records: Vec<_> = ParseIter::new(buffer).collect();  // iterate unindexed data again
    records.par_iter_mut().for_each(do_stuff);               // iterate indexed data in parallel

*/

pub enum QueryType {
    Q,   // query
    TUW, // url or title words
}

impl From<String> for QueryType {
    fn from(prefix: String) -> Self {
        match prefix.as_ref() {
            "qe" => QueryType::Q,
            "q" => QueryType::Q,
            _ => QueryType::TUW,
        }
    }
}


pub fn shard(
    file_path: &str,
    nr_shards: usize,
    output_dir: &str,
    nthreads: usize,
) -> Result<(), Error> {
    println!("Sharding...");

    // delete previous shards if they exist
    for i in 0..nr_shards {
        let ref shard_path = format!("{}/ngrams.{}", output_dir, i);
        remove_file_if_exists!(shard_path);
    }

    let c = config::Config::init(output_dir.to_string());

    let stopwords = match stopwords::load(&c.stopwords_path) {
        Ok(stopwords) => stopwords,
        Err(_) => panic!("Failed to load stop-words!"),
    };

    let (sender, receiver): (Sender<u64>, Receiver<u64>) = mpsc::channel();

    for i in 0..nthreads {
        let sender = sender.clone();
        let stopwords = stopwords.clone();

        let mut shards = vec![];
        for i in 0..nr_shards {
            let file = OpenOptions::new()
                .create(true)
                .append(true) // will be written by multiple threads
                .open(format!("{}/ngrams.{}", output_dir, i))
                .unwrap();

            let f = BufWriter::new(file);
            shards.push(f);
        }

        let mut shards_ngrams: HashMap<u32, String> = HashMap::new();

        let tr_map = match Map::from_path(&c.terms_relevance_path) {
            Ok(tr_map) => tr_map,
            Err(_) => panic!("Failed to load terms rel. map!"),
        };

        let f = try!(File::open(file_path));

        thread::spawn(move || {
            let mut line_count: u64 = 0;
            let reader = BufReader::with_capacity(5 * 1024 * 1024, &f);

            for (lnum, line) in reader.lines().enumerate() {
                if lnum % nthreads != i {
                    continue;
                }

                let qid = lnum as u64;

                let line = match line {
                    Ok(line) => line,
                    Err(e) => {
                        println!("Read line error: {:?}", e);
                        continue;
                    }
                };

                let mut v: Vec<&str> = line.split("\t").map(|t| t.trim()).collect();
                v = v[0].split(":").map(|t| t.trim()).collect();
                let (prefix, query) = match v.len() {
                    1 => ("qe".to_string(), v[0].to_string()),
                    2 => (v[0].to_string(), v[1].to_string()),
                    _ => (v[0].to_string(), v[1..v.len() - 1].join(" ")),
                };

                let ngrams = &ngrams::parse(&query, &stopwords, &tr_map, QueryType::from(prefix));

                for (ngram, sc) in ngrams {
                    let shard_id = util::jump_consistent_hash_str(ngram, nr_shards as u32);

                    // shard id to the query id
                    let (pqid, reminder) = util::qid2pqid(qid, nr_shards);
                    let qsc = (sc * 100.0).round() as u8;

                    // Note: writes u32 shard id for the query, not u64 query id, this is
                    // because a query id that is bigger than 2**32 overflows u64 in pairing
                    // function. When reading the shard id is used to get the original query id.
                    let line = format!("{}\t{}\t{}\t{}\n", pqid, reminder, ngram, qsc);

                    let sh_lines = shards_ngrams.entry(shard_id).or_insert(String::from(""));
                    *sh_lines = format!("{}{}", sh_lines, line);

                    if sh_lines.len() > WRITE_BUFFER_SIZE {
                        shards[shard_id as usize]
                            .write_all(sh_lines.as_bytes())
                            .expect("Unable to write data");

                        shards[shard_id as usize].flush().expect("Flush failed");

                        *sh_lines = String::from("");
                    }
                }

                line_count += 1;
                if line_count as u64 % 1_000_000 == 0 {
                    println!(
                        "Processed {:.1}M queries, thread {}",
                        line_count / 1_000_000,
                        i
                    );
                }
            }

            // write the reminder
            for (shard_id, lines) in shards_ngrams {
                if lines.len() > 0 {
                    shards[shard_id as usize]
                        .write_all(lines.as_bytes())
                        .expect("Unable to write data");

                    shards[shard_id as usize].flush().expect("Flush failed");
                }
            }

            sender.send(line_count).unwrap(); //finished!
        });
    }

    let mut line_count: u64 = 0;
    for _ in 0..nthreads {
        line_count += receiver.recv().unwrap();
    }
    println!("Total count of sharded queries {:.1}", line_count);

    Ok(())
}
